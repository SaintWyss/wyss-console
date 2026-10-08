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
        safety_profile: None,
        workspace_root: None,
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
        for d in ["run", "state", "bin", "codex-home"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(d))
                .unwrap();
        }
        // Backend metadata as the provider caches it: every tool selector the authority must pin.
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
    fn start(&self) -> Authority {
        Authority(
            Command::new(env!("CARGO_BIN_EXE_wyss-control"))
                .env("XDG_RUNTIME_DIR", self.root.join("run"))
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

#[test]
fn inference_generation_only_starts_with_the_authority_model_catalog() {
    let env = Sandbox::new("happy");
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(mutation()).unwrap();
    let ready = env.wait(&mut c, ActualState::Ready);
    assert!(ready.runtime_safety_profile.is_some());
    // Discovery has no catalog; only the thread-capable generation is launched with it.
    assert_eq!(
        fs::read_to_string(env.root.join("catalog-launches"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    let catalog = env.root.join("run/wyss-console/codex-model-catalog.json");
    assert_eq!(
        fs::metadata(&catalog).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let presented: serde_json::Value =
        serde_json::from_slice(&fs::read(&catalog).unwrap()).unwrap();
    let model = &presented["models"][0];
    assert_eq!(model["slug"], "fixture-model");
    assert_eq!(model["tool_mode"], "direct");
    assert_eq!(model["experimental_supported_tools"], serde_json::json!([]));
    assert!(model["multi_agent_version"].is_null() && model["apply_patch_tool_type"].is_null());
    a.stop(false);
    // Without a provable catalog there is no thread-capable generation at all.
    for sabotage in ["missing", "symlink", "malformed", "no_models"] {
        let env = Sandbox::new("happy");
        let cache = env.root.join("codex-home/models_cache.json");
        match sabotage {
            "missing" => fs::remove_file(&cache).unwrap(),
            "symlink" => {
                let real = env.root.join("elsewhere.json");
                fs::rename(&cache, &real).unwrap();
                std::os::unix::fs::symlink(&real, &cache).unwrap();
            }
            "malformed" => fs::write(&cache, b"{\"models\": [").unwrap(),
            _ => fs::write(&cache, br#"{"models":[]}"#).unwrap(),
        }
        let mut a = env.start();
        let mut c = env.client();
        c.runtime_start(mutation()).unwrap();
        let failed = env.wait(&mut c, ActualState::Failed);
        assert_eq!(
            failed.diagnostic,
            Some(RuntimeDiagnostic::SafetyUnproven),
            "{sabotage}"
        );
        assert!(!env.root.join("catalog-launches").exists(), "{sabotage}");
        assert_eq!(count(&env, "thread/start"), 0, "{sabotage}");
        a.stop(false);
    }
}

/// A workspace nested in a repository that carries project policy at two levels, and a user
/// Codex home holding a credential next to configuration and rules that must stay hidden.
fn local_layout(env: &Sandbox) -> String {
    let home = env.root.join("codex-home");
    fs::write(home.join("auth.json"), "FIXTURE-CREDENTIAL").unwrap();
    fs::set_permissions(home.join("auth.json"), fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(home.join("config.toml"), "approval_policy = \"never\"\n").unwrap();
    fs::create_dir_all(home.join("rules")).unwrap();
    fs::write(home.join("rules/allow.rules"), "allow everything\n").unwrap();
    for dir in [
        "repo/.git",
        "repo/.codex",
        "repo/sub/.codex/rules",
        "repo/sub/ws",
    ] {
        fs::create_dir_all(env.root.join(dir)).unwrap();
    }
    fs::write(
        env.root.join("repo/.codex/config.toml"),
        "approval_policy = \"never\"\n",
    )
    .unwrap();
    fs::write(
        env.root.join("repo/sub/.codex/rules/allow.rules"),
        "allow everything\n",
    )
    .unwrap();
    let layers = [
        env.root.join("repo/.codex"),
        env.root.join("repo/sub/.codex"),
    ];
    fs::write(
        env.root.join("layers.json"),
        serde_json::to_string(&layers).unwrap(),
    )
    .unwrap();
    env.root.join("repo/sub/ws").to_str().unwrap().to_owned()
}
fn local_mutation(workspace: &str) -> RuntimeMutation {
    RuntimeMutation {
        safety_profile: Some(RuntimeSafetyProfile::CodexLocalToolsV1),
        workspace_root: Some(workspace.to_owned()),
        ..mutation()
    }
}
fn local_scopes() -> Vec<ExecutionScope> {
    vec![
        ExecutionScope::SessionCreate,
        ExecutionScope::SessionResume,
        ExecutionScope::TurnStart,
        ExecutionScope::CommandExecute,
        ExecutionScope::FileChange,
    ]
}
fn observed(env: &Sandbox) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(env.root.join("local.json")).unwrap()).unwrap()
}
#[test]
fn local_tools_generation_is_isolated_attested_and_serves_only_its_workspace() {
    let env = Sandbox::new("happy");
    let workspace = local_layout(&env);
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(local_mutation(&workspace)).unwrap();
    let ready = env.wait(&mut c, ActualState::Ready);
    assert_eq!(
        ready.runtime_safety_profile,
        Some(RuntimeSafetyProfile::CodexLocalToolsV1)
    );
    assert_eq!(ready.workspace_root.as_deref(), Some(workspace.as_str()));
    // What the runtime process itself could see from inside its namespace.
    let seen = observed(&env);
    let private = env.root.join("state/wyss-console/codex-local-tools");
    assert_eq!(seen["codex_home"], private.join("home").to_str().unwrap());
    assert_eq!(seen["requirements"], "[features]\nunified_exec = false\n");
    assert_eq!(
        seen["managed_entries"],
        serde_json::json!(["managed_config.toml", "requirements.toml"])
    );
    assert_eq!(seen["user_home_entries"], serde_json::json!([]));
    assert_eq!(seen["credential"], "FIXTURE-CREDENTIAL");
    assert_eq!(seen["credential_writable"], true);
    assert_eq!(seen["inherited"], serde_json::json!(["CODEX_HOME"]));
    assert_eq!(seen["layers_writable"], serde_json::json!([]));
    for (layer, entries) in seen["layers"].as_object().unwrap() {
        assert_eq!(entries, &serde_json::json!([]), "{layer} is masked");
    }
    assert_eq!(seen["layers"].as_object().unwrap().len(), 2);
    // Outside it nothing was hidden, copied or created.
    for (file, content) in [
        ("codex-home/auth.json", "FIXTURE-CREDENTIAL"),
        ("codex-home/config.toml", "approval_policy = \"never\"\n"),
        ("repo/.codex/config.toml", "approval_policy = \"never\"\n"),
        ("repo/sub/.codex/rules/allow.rules", "allow everything\n"),
    ] {
        assert_eq!(fs::read_to_string(env.root.join(file)).unwrap(), content);
    }
    assert!(!env.root.join("repo/sub/ws/.codex").exists());
    assert_eq!(
        fs::metadata(private.join("home/auth.json")).unwrap().len(),
        0
    );
    // Both the discovery process and the final generation ran inside the namespace, and the
    // final one proved its managed requirement.
    assert_eq!(
        fs::read_to_string(env.root.join("local-launches")).unwrap(),
        "discovery\nfinal\n"
    );
    assert_eq!(
        env.methods(),
        [
            "initialize",
            "initialized",
            "model/list",
            "config/read",
            "initialize",
            "initialized",
            "model/list",
            "config/read",
            "configRequirements/read",
            "hooks/list",
        ]
    );
    assert_eq!(ready.outbound_methods.config_requirements_read, 1);
    let catalog = env
        .root
        .join("run/wyss-console/codex-local-tools-catalog.json");
    assert_eq!(
        fs::metadata(&catalog).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let presented: serde_json::Value =
        serde_json::from_slice(&fs::read(&catalog).unwrap()).unwrap();
    let model = &presented["models"][0];
    assert_eq!(model["tool_mode"], "direct");
    assert_eq!(model["experimental_supported_tools"], serde_json::json!([]));
    assert!(model["multi_agent_version"].is_null());
    assert_eq!(model["apply_patch_tool_type"], "freeform");
    assert!(
        !env.root
            .join("run/wyss-console/codex-model-catalog.json")
            .exists()
    );
    // A generation never changes profile or workspace: that needs an explicit stop.
    let other = env.root.join("repo/sub");
    for refused in [mutation(), local_mutation(other.to_str().unwrap())] {
        assert_eq!(
            c.runtime_start(refused),
            Err(ControlError::InvalidTransition)
        );
    }
    let before = c.runtime_status().unwrap();
    assert_eq!(
        before.revision, ready.revision,
        "a refused start changes nothing durable"
    );
    let pids = env.pids();
    c.runtime_start(local_mutation(&workspace)).unwrap();
    assert_eq!(
        env.pids(),
        pids,
        "the same request keeps the same generation"
    );
    // Sessions attach only for that workspace, under the mediated posture.
    let (s, _) = prepare_bound_session(&mut c, local_scopes());
    let elsewhere = BindingCreate {
        workspace_root: env.root.to_str().unwrap().to_owned(),
        ..create_params(&env, &s)
    };
    assert_eq!(
        c.binding_create(elsewhere),
        Err(ControlError::WorkspaceInvalid)
    );
    assert_eq!(count(&env, "thread/start"), 0);
    let created = c
        .binding_create(BindingCreate {
            workspace_root: workspace.clone(),
            ..create_params(&env, &s)
        })
        .unwrap();
    assert_eq!(created.workspace_root, workspace);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            &fs::read_to_string(env.root.join("trust-roots")).unwrap()
        )
        .unwrap(),
        serde_json::json!([env.root.join("repo").to_str().unwrap(), workspace.as_str()])
    );
    c.binding_resume(resume_params(&s)).unwrap();
    let attached = c.binding_status(s.managed_session_id.clone()).unwrap();
    assert_eq!(attached.state, BindingState::BoundAttached);
    assert_eq!(
        attached.attachment_profile,
        Some(AttachmentProfile::LocalToolsV1)
    );
    for action in local_scopes().into_iter().skip(2) {
        let gate = c
            .gate_status(GateQuery {
                session_id: s.managed_session_id.clone(),
                action,
            })
            .unwrap();
        assert_eq!(gate.reason, GateReason::Allowed, "{action:?}");
    }
    // Stopping and starting without a profile gives the inference generation again, and
    // neither it nor a restarted authority grants what only the isolated profile may.
    c.runtime_stop(mutation()).unwrap();
    env.wait(&mut c, ActualState::Stopped);
    c.runtime_start(mutation()).unwrap();
    let inference = env.wait(&mut c, ActualState::Ready);
    assert_eq!(
        (
            inference.runtime_safety_profile,
            inference.workspace_root.clone()
        ),
        (Some(RuntimeSafetyProfile::CodexInferenceV1), None)
    );
    for action in [ExecutionScope::CommandExecute, ExecutionScope::FileChange] {
        let gate = c
            .gate_status(GateQuery {
                session_id: s.managed_session_id.clone(),
                action,
            })
            .unwrap();
        assert_eq!(gate.reason, GateReason::LocalToolsProfileMissing);
    }
    c.runtime_stop(mutation()).unwrap();
    env.wait(&mut c, ActualState::Stopped);
    c.runtime_start(local_mutation(&workspace)).unwrap();
    env.wait(&mut c, ActualState::Ready);
    drop(c);
    a.stop(false);
    for pid in env.pids() {
        dead(pid);
    }
    // Nothing durable records the profile: a desired-running runtime comes back as the
    // least capable one.
    let mut a = env.start();
    let mut c = env.client();
    let restarted = env.wait(&mut c, ActualState::Ready);
    assert_eq!(
        restarted.runtime_safety_profile,
        Some(RuntimeSafetyProfile::CodexInferenceV1)
    );
    drop(c);
    a.stop(false);
}
#[test]
fn local_tools_readiness_fails_closed_whenever_isolation_or_attestation_is_unproven() {
    // The runtime's own answers do not prove the profile.
    for (scenario, final_launch) in [
        ("local_wrong_home", false),
        ("local_mcp", false),
        ("local_no_requirement", true),
        ("local_tmp_writable", true),
    ] {
        let env = Sandbox::new(scenario);
        let workspace = local_layout(&env);
        let mut a = env.start();
        let mut c = env.client();
        c.runtime_start(local_mutation(&workspace)).unwrap();
        let failed = env.wait(&mut c, ActualState::Failed);
        assert_eq!(
            failed.diagnostic,
            Some(RuntimeDiagnostic::SafetyUnproven),
            "{scenario}"
        );
        assert_eq!(
            (failed.runtime_safety_profile, failed.workspace_root),
            (None, None)
        );
        assert_eq!(
            fs::read_to_string(env.root.join("local-launches"))
                .unwrap()
                .contains("final"),
            final_launch,
            "{scenario}"
        );
        assert_eq!(count(&env, "thread/start") + count(&env, "turn/start"), 0);
        let (s, _) = prepare_bound_session(&mut c, local_scopes());
        assert_eq!(
            c.binding_create(BindingCreate {
                workspace_root: workspace.clone(),
                ..create_params(&env, &s)
            }),
            Err(ControlError::GateDenied)
        );
        drop(c);
        a.stop(false);
        for pid in env.pids() {
            dead(pid);
        }
    }
    // The host does not allow a safe namespace: no runtime process is ever launched.
    for sabotage in [
        "credential_mode",
        "credential_symlink",
        "layer_symlink",
        "layer_file",
    ] {
        let env = Sandbox::new("happy");
        let workspace = local_layout(&env);
        let auth = env.root.join("codex-home/auth.json");
        let layer = env.root.join("repo/sub/ws/.codex");
        match sabotage {
            "credential_mode" => {
                fs::set_permissions(&auth, fs::Permissions::from_mode(0o644)).unwrap()
            }
            "credential_symlink" => {
                let real = env.root.join("real-auth.json");
                fs::rename(&auth, &real).unwrap();
                std::os::unix::fs::symlink(&real, &auth).unwrap();
            }
            "layer_symlink" => {
                std::os::unix::fs::symlink(env.root.join("repo/.codex"), &layer).unwrap()
            }
            _ => fs::write(&layer, "not a directory").unwrap(),
        }
        let mut a = env.start();
        let mut c = env.client();
        c.runtime_start(local_mutation(&workspace)).unwrap();
        let failed = env.wait(&mut c, ActualState::Failed);
        assert_eq!(
            failed.diagnostic,
            Some(RuntimeDiagnostic::SafetyUnproven),
            "{sabotage}"
        );
        assert!(!env.root.join("local-launches").exists(), "{sabotage}");
        assert!(env.methods().is_empty(), "{sabotage}");
        drop(c);
        a.stop(false);
    }
    // A request that names no usable workspace is refused before anything durable changes.
    let env = Sandbox::new("happy");
    let workspace = local_layout(&env);
    let mut a = env.start();
    let mut c = env.client();
    let alias = env.root.join("alias");
    std::os::unix::fs::symlink(&workspace, &alias).unwrap();
    let invalid = [
        (Some(RuntimeSafetyProfile::CodexLocalToolsV1), None),
        (None, Some(workspace.clone())),
        (
            Some(RuntimeSafetyProfile::CodexInferenceV1),
            Some(workspace.clone()),
        ),
        (
            Some(RuntimeSafetyProfile::CodexLocalToolsV1),
            Some("relative/ws".to_owned()),
        ),
        (
            Some(RuntimeSafetyProfile::CodexLocalToolsV1),
            Some(env.root.join("missing").to_str().unwrap().to_owned()),
        ),
    ];
    for (safety_profile, workspace_root) in invalid {
        let request = RuntimeMutation {
            safety_profile,
            workspace_root,
            ..mutation()
        };
        assert!(c.runtime_start(request).is_err());
    }
    let stop = RuntimeMutation {
        safety_profile: Some(RuntimeSafetyProfile::CodexLocalToolsV1),
        workspace_root: Some(workspace.clone()),
        ..mutation()
    };
    assert!(c.runtime_stop(stop).is_err());
    let untouched = c.runtime_status().unwrap();
    assert_eq!(
        (untouched.revision, untouched.actual_state),
        (0, ActualState::Stopped)
    );
    assert!(env.pids().is_empty());
    // A symlinked spelling of the workspace is accepted only as its canonical path.
    c.runtime_start(local_mutation(alias.to_str().unwrap()))
        .unwrap();
    let ready = env.wait(&mut c, ActualState::Ready);
    assert_eq!(ready.workspace_root.as_deref(), Some(workspace.as_str()));
    drop(c);
    a.stop(false);
}
#[test]
fn local_tools_thread_posture_is_verified_and_new_project_policy_stops_further_work() {
    for scenario in ["local_weak_thread", "local_network_thread"] {
        let env = Sandbox::new(scenario);
        let workspace = local_layout(&env);
        let mut a = env.start();
        let mut c = env.client();
        c.runtime_start(local_mutation(&workspace)).unwrap();
        env.wait(&mut c, ActualState::Ready);
        let (s, _) = prepare_bound_session(&mut c, local_scopes());
        let create = BindingCreate {
            workspace_root: workspace.clone(),
            ..create_params(&env, &s)
        };
        assert!(c.binding_create(create).is_err(), "{scenario}");
        let status = c.binding_status(s.managed_session_id.clone()).unwrap();
        assert_ne!(status.state, BindingState::BoundAttached);
        assert_eq!(status.attachment_profile, None);
        // An answer that does not prove the posture leaves an uncertain runtime: contained.
        env.wait(&mut c, ActualState::Failed);
        assert_eq!(count(&env, "turn/start"), 0);
        drop(c);
        a.stop(false);
    }
    let env = Sandbox::new("happy");
    let workspace = local_layout(&env);
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(local_mutation(&workspace)).unwrap();
    env.wait(&mut c, ActualState::Ready);
    // The runtime's own sandbox leaves an empty protective directory: that is not policy.
    let layer = env.root.join("repo/sub/ws/.codex");
    fs::create_dir(&layer).unwrap();
    let (s, _) = prepare_bound_session(&mut c, local_scopes());
    c.binding_create(BindingCreate {
        workspace_root: workspace.clone(),
        ..create_params(&env, &s)
    })
    .unwrap();
    // Policy that this namespace does not mask appeared after it was created.
    fs::write(layer.join("config.toml"), "approval_policy = \"never\"\n").unwrap();
    let frames = (count(&env, "thread/resume"), count(&env, "turn/start"));
    assert!(c.binding_resume(resume_params(&s)).is_err());
    assert_eq!(
        (count(&env, "thread/resume"), count(&env, "turn/start")),
        frames
    );
    env.wait(&mut c, ActualState::Failed);
    assert_ne!(
        c.binding_status(s.managed_session_id.clone())
            .unwrap()
            .state,
        BindingState::BoundAttached
    );
    drop(c);
    a.stop(false);
}
#[test]
fn local_tools_generation_executes_nothing_while_no_approval_can_be_answered() {
    for scenario in [
        "isolation_approval",
        "isolation_commandExecution",
        "isolation_fileChange",
        "isolation_permission",
        "isolation_mcpToolCall",
        "isolation_collabAgentToolCall",
    ] {
        let env = Sandbox::new(scenario);
        let workspace = local_layout(&env);
        let mut a = env.start();
        let mut c = env.client();
        c.runtime_start(local_mutation(&workspace)).unwrap();
        env.wait(&mut c, ActualState::Ready);
        let (s, _) = prepare_bound_session(&mut c, local_scopes());
        c.binding_create(BindingCreate {
            workspace_root: workspace.clone(),
            ..create_params(&env, &s)
        })
        .unwrap();
        c.binding_resume(resume_params(&s)).unwrap();
        let t = c.turn_start(turn_params(&s)).unwrap();
        env.wait(&mut c, ActualState::Failed);
        let failed = wait_turn(&mut c, &t, true);
        assert_eq!(failed.state, TurnState::Failed, "{scenario}");
        assert_eq!(
            failed.terminal_reason,
            Some(TurnReason::InferenceIsolationViolation),
            "{scenario}"
        );
        // No provider request was ever answered: nothing was accepted, so nothing ran.
        assert_eq!(count(&env, "RESPONSE"), 0, "{scenario}");
        assert_eq!(count(&env, "turn/start"), 1);
        a.stop(false);
        for pid in env.pids() {
            dead(pid);
        }
    }
}
/// Every approval event of the durable audit stream, in order.
fn approval_events(c: &mut ControlClient) -> Vec<serde_json::Value> {
    let stream = c.hello_status().stream_id.clone();
    let mut after = None;
    let mut found = Vec::new();
    loop {
        let ResultValue::Audit { events, next } = c
            .audit(AuditRead {
                stream_id: stream.clone(),
                after_sequence: after,
                limit: 50,
            })
            .unwrap()
        else {
            panic!()
        };
        for event in &events {
            let value = serde_json::to_value(event).unwrap();
            if value["event_type"]
                .as_str()
                .unwrap()
                .starts_with("authority.managed_approval.")
            {
                found.push(value);
            }
        }
        if next.is_none() {
            return found;
        }
        after = next;
    }
}
fn wait_approval(c: &mut ControlClient, kind: &str) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let events = approval_events(c);
        if let Some(event) = events
            .iter()
            .find(|e| e["event_type"] == format!("authority.managed_approval.{kind}"))
        {
            return event.clone();
        }
        assert!(Instant::now() < deadline, "no {kind} approval: {events:?}");
        thread::sleep(Duration::from_millis(5));
    }
}
/// The decision of every response frame the authority wrote to the provider.
fn decisions(env: &Sandbox) -> Vec<String> {
    fs::read_to_string(env.root.join("decisions"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}
/// What an executed command or an applied patch would have left behind.
fn effects(env: &Sandbox, workspace: &str) -> usize {
    ["executions", "mutations"]
        .iter()
        .filter(|name| env.root.join(name).exists())
        .count()
        + usize::from(
            PathBuf::from(workspace)
                .join("wyss-fixture-probe.txt")
                .exists(),
        )
}
fn local_session(
    env: &Sandbox,
    c: &mut ControlClient,
    workspace: &str,
    scopes: Vec<ExecutionScope>,
) -> (ManagedSessionRecord, ExecutionLease) {
    let (s, l) = prepare_bound_session(c, scopes);
    c.binding_create(BindingCreate {
        workspace_root: workspace.to_owned(),
        ..create_params(env, &s)
    })
    .unwrap();
    c.binding_resume(resume_params(&s)).unwrap();
    (s, l)
}
#[test]
fn local_tools_approval_request_is_admitted_pending_and_only_ever_refused_without_a_decision() {
    nix::sys::prctl::set_child_subreaper(true).unwrap();
    for (scenario, kind) in [
        ("approval_command", "command_execution"),
        ("approval_file", "file_change"),
    ] {
        for cause in ["cancel", "revoke", "expiry", "close", "stop", "restart"] {
            let env = Sandbox::new(scenario);
            let workspace = local_layout(&env);
            let mut a = env.start();
            let mut c = env.client();
            c.runtime_start(local_mutation(&workspace)).unwrap();
            env.wait(&mut c, ActualState::Ready);
            let (s, l) = local_session(&env, &mut c, &workspace, local_scopes());
            let lease = if cause == "expiry" {
                c.lease_renew(LeaseRenew {
                    intent_id: id(),
                    lease_id: l.lease_id.clone(),
                    expected_revision: l.revision,
                    ttl_seconds: 2,
                })
                .unwrap()
            } else {
                l
            };
            let t = c.turn_start(turn_params(&s)).unwrap();
            let requested = wait_approval(&mut c, "requested");
            let label = format!("{scenario}/{cause}");
            let payload = &requested["payload"];
            assert_eq!(payload["kind"], kind, "{label}");
            assert_eq!(payload["state"], "pending");
            assert_eq!(payload["managed_turn_id"], t.managed_turn_id.as_str());
            // The ORIGINAL lease of the turn, as admitted.
            assert_eq!(payload["lease_id"], lease.lease_id.as_str());
            assert_eq!(payload["lease_revision"], lease.revision);
            assert_eq!(payload["action_fingerprint"].as_str().unwrap().len(), 64);
            assert_eq!(requested["session_id"], s.managed_session_id.as_str());
            // Pending means exactly that: no answer was written, so nothing ran.
            let running = wait_turn(&mut c, &t, false);
            thread::sleep(Duration::from_millis(150));
            assert!(decisions(&env).is_empty(), "{label}");
            assert_eq!(effects(&env, &workspace), 0, "{label}");
            match cause {
                "cancel" => {
                    c.turn_cancel(TurnCancel {
                        intent_id: id(),
                        managed_turn_id: t.managed_turn_id.clone(),
                        expected_revision: running.revision,
                    })
                    .unwrap();
                }
                "revoke" => {
                    c.lease_revoke(LeaseRevoke {
                        intent_id: id(),
                        lease_id: lease.lease_id.clone(),
                        expected_revision: lease.revision,
                    })
                    .unwrap();
                }
                "close" => {
                    c.close(Close {
                        intent_id: id(),
                        managed_session_id: s.managed_session_id.clone(),
                        expected_revision: 1,
                    })
                    .unwrap();
                }
                "stop" => {
                    c.runtime_stop(mutation()).unwrap();
                }
                "restart" => {
                    a.stop(true);
                    for pid in env.pids() {
                        dead(pid);
                    }
                    a = env.start();
                    c = env.client();
                    env.wait(&mut c, ActualState::Ready);
                }
                _ => (),
            }
            let done = wait_turn(&mut c, &t, true);
            let (state, reason, approval, why, answers): (_, _, _, _, &[&str]) = match cause {
                "cancel" => (
                    TurnState::Interrupted,
                    TurnReason::UserCancel,
                    "invalidated",
                    "turn_cancelled",
                    &["\"cancel\""],
                ),
                "revoke" => (
                    TurnState::Interrupted,
                    TurnReason::LeaseRevoked,
                    "invalidated",
                    "lease_revoked",
                    &["\"cancel\""],
                ),
                "expiry" => (
                    TurnState::Interrupted,
                    TurnReason::LeaseExpired,
                    "expired",
                    "lease_expired",
                    &["\"cancel\""],
                ),
                "close" => (
                    TurnState::Interrupted,
                    TurnReason::SessionClosed,
                    "invalidated",
                    "session_closed",
                    &["\"cancel\""],
                ),
                "stop" => (
                    TurnState::Indeterminate,
                    TurnReason::RuntimeStopped,
                    "invalidated",
                    "runtime_lost",
                    &[],
                ),
                _ => (
                    TurnState::Indeterminate,
                    TurnReason::AuthorityRestarted,
                    "invalidated",
                    "authority_restarted",
                    &[],
                ),
            };
            assert_eq!(
                (done.state, done.terminal_reason),
                (state, Some(reason)),
                "{label}"
            );
            let settled = wait_approval(&mut c, approval);
            assert_eq!(settled["payload"]["reason"], why, "{label}");
            assert!(settled["payload"]["decision"].is_null(), "{label}");
            assert_eq!(settled["payload"]["effect"], "none", "{label}");
            assert_eq!(
                settled["payload"]["action_fingerprint"],
                payload["action_fingerprint"]
            );
            assert_eq!(approval_events(&mut c).len(), 2, "{label}");
            // Exactly the reductive answer, or none at all; never an accept, never twice,
            // never an interrupt on top of it, and nothing ran.
            assert_eq!(decisions(&env), answers, "{label}");
            assert_eq!(count(&env, "RESPONSE"), answers.len(), "{label}");
            assert_eq!(count(&env, "turn/interrupt"), 0, "{label}");
            assert_eq!(count(&env, "turn/start"), 1, "{label}");
            assert_eq!(effects(&env, &workspace), 0, "{label}");
            // The audit stream holds control metadata only.
            let audit = serde_json::to_string(&approval_events(&mut c)).unwrap();
            for transient in ["WYSS_FIXTURE", "printf", "wyss-fixture-probe"] {
                assert!(!audit.contains(transient), "{label}");
            }
            drop(c);
            a.stop(false);
            for pid in env.pids() {
                dead(pid);
            }
        }
    }
}
#[test]
fn local_tools_request_beyond_the_one_announced_action_is_contained_and_never_answered() {
    for (scenario, opened) in [
        ("approval_command_stdin", false),
        ("approval_command_permissions", false),
        ("approval_command_network", false),
        ("approval_command_remote", false),
        ("approval_file_grant", false),
        ("approval_command_unasked", false),
        ("approval_command_double", true),
        ("approval_command_early", true),
    ] {
        let env = Sandbox::new(scenario);
        let workspace = local_layout(&env);
        let mut a = env.start();
        let mut c = env.client();
        c.runtime_start(local_mutation(&workspace)).unwrap();
        env.wait(&mut c, ActualState::Ready);
        let (s, _) = local_session(&env, &mut c, &workspace, local_scopes());
        let t = c.turn_start(turn_params(&s)).unwrap();
        let failed = env.wait(&mut c, ActualState::Failed);
        assert_eq!(
            failed.diagnostic,
            Some(RuntimeDiagnostic::InferenceIsolationViolation),
            "{scenario}"
        );
        let turn = wait_turn(&mut c, &t, true);
        assert_eq!(
            (turn.state, turn.terminal_reason),
            (
                TurnState::Failed,
                Some(TurnReason::InferenceIsolationViolation)
            ),
            "{scenario}"
        );
        // Never approved, never answered: the request that was legal is void as well.
        assert!(decisions(&env).is_empty(), "{scenario}");
        assert_eq!(count(&env, "RESPONSE"), 0, "{scenario}");
        let events = approval_events(&mut c);
        if opened {
            assert_eq!(events.len(), 2, "{scenario}");
            assert_eq!(
                events[1]["event_type"],
                "authority.managed_approval.invalidated"
            );
            assert_eq!(events[1]["payload"]["reason"], "isolation_violation");
        } else {
            assert!(events.is_empty(), "{scenario}: {events:?}");
        }
        assert_eq!(
            c.binding_status(s.managed_session_id).unwrap().state,
            BindingState::BoundDetached
        );
        drop(c);
        a.stop(false);
        for pid in env.pids() {
            dead(pid);
        }
    }
}
#[test]
fn local_tools_unacceptable_proposal_is_refused_reductively_without_an_approval() {
    // A command outside the workspace can never be accepted: the provider is refused,
    // the turn stops and the runtime stays healthy.
    let env = Sandbox::new("approval_command_outside");
    let workspace = local_layout(&env);
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(local_mutation(&workspace)).unwrap();
    env.wait(&mut c, ActualState::Ready);
    let (s, _) = local_session(&env, &mut c, &workspace, local_scopes());
    let t = c.turn_start(turn_params(&s)).unwrap();
    let done = wait_turn(&mut c, &t, true);
    assert_eq!(
        (done.state, done.terminal_reason),
        (TurnState::Interrupted, Some(TurnReason::ActionRejected))
    );
    assert!(approval_events(&mut c).is_empty());
    assert_eq!(decisions(&env), ["\"cancel\""]);
    assert_eq!(effects(&env, &workspace), 0);
    assert_eq!(c.runtime_status().unwrap().actual_state, ActualState::Ready);
    drop(c);
    a.stop(false);

    // A request that offers no refusal is never answered: the turn is interrupted instead.
    let env = Sandbox::new("approval_command_nocancel");
    let workspace = local_layout(&env);
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(local_mutation(&workspace)).unwrap();
    env.wait(&mut c, ActualState::Ready);
    let (s, _) = local_session(&env, &mut c, &workspace, local_scopes());
    let t = c.turn_start(turn_params(&s)).unwrap();
    wait_approval(&mut c, "requested");
    let running = wait_turn(&mut c, &t, false);
    c.turn_cancel(TurnCancel {
        intent_id: id(),
        managed_turn_id: t.managed_turn_id.clone(),
        expected_revision: running.revision,
    })
    .unwrap();
    assert_eq!(wait_turn(&mut c, &t, true).state, TurnState::Interrupted);
    assert_eq!(
        wait_approval(&mut c, "invalidated")["payload"]["reason"],
        "turn_cancelled"
    );
    assert!(decisions(&env).is_empty());
    assert_eq!(count(&env, "turn/interrupt"), 1);
    assert_eq!(effects(&env, &workspace), 0);
    assert_eq!(c.runtime_status().unwrap().actual_state, ActualState::Ready);
    drop(c);
    a.stop(false);
}
#[test]
fn inference_generation_answers_no_approval_request_even_when_well_formed() {
    for scenario in ["approval_command", "approval_file"] {
        let env = Sandbox::new(scenario);
        let mut a = env.start();
        let mut c = env.client();
        c.runtime_start(mutation()).unwrap();
        env.wait(&mut c, ActualState::Ready);
        let (s, _) = inference_session(&env, &mut c);
        let t = c.turn_start(turn_params(&s)).unwrap();
        env.wait(&mut c, ActualState::Failed);
        let failed = wait_turn(&mut c, &t, true);
        assert_eq!(
            failed.terminal_reason,
            Some(TurnReason::InferenceIsolationViolation),
            "{scenario}"
        );
        assert!(approval_events(&mut c).is_empty(), "{scenario}");
        assert!(decisions(&env).is_empty(), "{scenario}");
        assert_eq!(count(&env, "RESPONSE"), 0, "{scenario}");
        assert_eq!(effects(&env, env.root.to_str().unwrap()), 0);
        a.stop(false);
        for pid in env.pids() {
            dead(pid);
        }
    }
}
use wyss_console::contracts::identities::ManagedApprovalId;
/// A local-tools generation with one attached session whose turn waits on an approval.
struct Pending {
    env: Sandbox,
    authority: Authority,
    client: ControlClient,
    workspace: String,
    session: ManagedSessionRecord,
    lease: ExecutionLease,
    turn: ManagedTurnRecord,
    shown: ApprovalStatus,
}
fn pending(scenario: &'static str, scopes: Vec<ExecutionScope>) -> Pending {
    let env = Sandbox::new(scenario);
    let workspace = local_layout(&env);
    let authority = env.start();
    let mut client = env.client();
    client.runtime_start(local_mutation(&workspace)).unwrap();
    env.wait(&mut client, ActualState::Ready);
    let (session, lease) = local_session(&env, &mut client, &workspace, scopes);
    let turn = client.turn_start(turn_params(&session)).unwrap();
    wait_approval(&mut client, "requested");
    let shown = client
        .approval_pending(turn.managed_turn_id.clone())
        .unwrap()
        .expect("pending approval");
    Pending {
        env,
        authority,
        client,
        workspace,
        session,
        lease,
        turn,
        shown,
    }
}
impl Pending {
    fn intent(&self, decision: ApprovalDecision) -> ApprovalRespond {
        ApprovalRespond {
            intent_id: id(),
            managed_approval_id: self.shown.approval.managed_approval_id.clone(),
            expected_revision: self.shown.approval.revision,
            decision,
        }
    }
    fn status(&mut self) -> ApprovalStatus {
        self.client
            .approval_status(ApprovalGet {
                managed_approval_id: self.shown.approval.managed_approval_id.clone(),
                file_index: None,
                offset: None,
            })
            .unwrap()
    }
    fn stream(&mut self) -> Vec<ManagedTurnStreamEventV1> {
        let mut events = Vec::new();
        let mut after = None;
        loop {
            let page = self
                .client
                .turn_events(TurnEvents {
                    managed_turn_id: self.turn.managed_turn_id.clone(),
                    after_sequence: after,
                    limit: 50,
                })
                .unwrap();
            if page.events.is_empty() {
                return events;
            }
            after = page.next_sequence;
            events.extend(page.events);
        }
    }
    /// Nothing the provider or the client said about the action is durable.
    fn assert_private(&self, transient: &[&str]) {
        for name in [
            "control.sqlite3",
            "control.sqlite3-wal",
            "control.sqlite3-shm",
        ] {
            let bytes =
                fs::read(self.env.root.join("state/wyss-console").join(name)).unwrap_or_default();
            for raw in transient {
                assert!(
                    !bytes.windows(raw.len()).any(|w| w == raw.as_bytes()),
                    "{raw} is durable in {name}"
                );
            }
        }
    }
    fn finish(mut self) {
        drop(self.client);
        self.authority.stop(false);
        for pid in self.env.pids() {
            dead(pid);
        }
    }
}
fn lines(env: &Sandbox, name: &str) -> usize {
    fs::read_to_string(env.root.join(name))
        .unwrap_or_default()
        .lines()
        .count()
}
#[test]
fn approval_protocol_accepts_one_command_exactly_once_after_attestation() {
    nix::sys::prctl::set_child_subreaper(true).unwrap();
    let mut p = pending("approval_command", local_scopes());
    // The presentation is the authority's own: the complete command, never a provider frame.
    assert!(p.shown.acceptable);
    assert_eq!(p.shown.approval.kind, ApprovalKind::CommandExecution);
    let command = p.shown.command.as_ref().unwrap();
    assert_eq!(command.command, "printf WYSS_FIXTURE_COMMAND");
    assert_eq!(command.cwd, p.workspace);
    assert_eq!(
        command.parsed_actions,
        ["unknown: printf WYSS_FIXTURE_COMMAND"]
    );
    // Another connection sees the same approval; closing it decides nothing.
    let mut other = p.env.client();
    assert!(
        other
            .approval_pending(p.turn.managed_turn_id.clone())
            .unwrap()
            .unwrap()
            == p.shown
    );
    drop(other);
    thread::sleep(Duration::from_millis(200));
    assert_eq!(p.status().approval, p.shown.approval);
    assert!(decisions(&p.env).is_empty());
    assert_eq!(effects(&p.env, &p.workspace), 0);
    // Intents that cannot be about this pending approval change nothing.
    let mut stale = p.intent(ApprovalDecision::Accept);
    stale.expected_revision += 1;
    assert_eq!(
        p.client.approval_respond(stale),
        Err(ControlError::StaleRevision)
    );
    let mut missing = p.intent(ApprovalDecision::Accept);
    missing.managed_approval_id = ManagedApprovalId::parse(id().as_str()).unwrap();
    assert_eq!(
        p.client.approval_respond(missing),
        Err(ControlError::ApprovalNotFound)
    );
    assert!(decisions(&p.env).is_empty());
    let before = p.env.methods().len();
    let accept = p.intent(ApprovalDecision::Accept);
    let receipt = p.client.approval_respond(accept.clone()).unwrap();
    assert_eq!(
        (receipt.decision, receipt.state, receipt.revision),
        (
            ApprovalDecision::Accept,
            ApprovalState::Accepted,
            p.shown.approval.revision + 1
        )
    );
    assert_eq!(
        receipt.action_fingerprint,
        p.shown.approval.action_fingerprint
    );
    let done = wait_turn(&mut p.client, &p.turn, true);
    assert_eq!(done.state, TurnState::Completed);
    // The runtime re-proved its profile, and only then was exactly one frame written.
    assert_eq!(
        p.env.methods()[before..],
        [
            "config/read",
            "configRequirements/read",
            "hooks/list",
            "RESPONSE"
        ]
    );
    assert_eq!(decisions(&p.env), ["\"accept\""]);
    assert_eq!(lines(&p.env, "executions"), 1);
    // The same intent, from any connection, returns the original receipt and no frame.
    assert_eq!(
        p.client.approval_respond(accept.clone()),
        Ok(receipt.clone())
    );
    let mut reconnected = p.env.client();
    assert_eq!(reconnected.approval_respond(accept.clone()), Ok(receipt));
    let mut conflict = accept.clone();
    conflict.decision = ApprovalDecision::Cancel;
    assert_eq!(
        reconnected.approval_respond(conflict),
        Err(ControlError::IntentConflict)
    );
    let mut again = p.intent(ApprovalDecision::Accept);
    again.expected_revision += 1;
    assert_eq!(
        reconnected.approval_respond(again),
        Err(ControlError::InvalidTransition)
    );
    drop(reconnected);
    assert_eq!(decisions(&p.env), ["\"accept\""]);
    assert_eq!(count(&p.env, "RESPONSE"), 1);
    assert_eq!(lines(&p.env, "executions"), 1);
    let methods = p.client.runtime_status().unwrap().outbound_methods;
    assert_eq!(
        (
            methods.approval_accept,
            methods.approval_decline,
            methods.approval_cancel,
            methods.turn_interrupt
        ),
        (1, 0, 0, 0)
    );
    // Canonical journal: requested, accepted with the dispatch marker, confirmed.
    let journal: Vec<_> = approval_events(&mut p.client)
        .iter()
        .map(|e| {
            (
                e["event_type"]
                    .as_str()
                    .unwrap()
                    .rsplit('.')
                    .next()
                    .unwrap()
                    .to_owned(),
                e["payload"]["effect"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        journal,
        [
            ("requested".to_owned(), "none".to_owned()),
            ("accepted".to_owned(), "dispatched".to_owned()),
            ("confirmed".to_owned(), "confirmed".to_owned()),
        ]
    );
    let settled = p.status();
    assert_eq!(settled.approval.effect, ApprovalEffect::Confirmed);
    assert!(!settled.acceptable && settled.command.is_none());
    assert!(
        p.client
            .approval_pending(p.turn.managed_turn_id.clone())
            .unwrap()
            .is_none()
    );
    // The one transient stream reports the whole lifecycle with the output and without
    // the command.
    let events = p.stream();
    let lifecycle: Vec<_> = events
        .iter()
        .map(|e| e.kind)
        .filter(|k| !matches!(k, StreamKind::AssistantMessageDelta))
        .collect();
    assert_eq!(
        lifecycle,
        [
            StreamKind::TurnStarted,
            StreamKind::CommandProposed,
            StreamKind::ApprovalRequired,
            StreamKind::ApprovalResolved,
            StreamKind::CommandStarted,
            StreamKind::CommandOutputDelta,
            StreamKind::CommandCompleted,
            StreamKind::TurnCompleted,
        ]
    );
    assert!(events.windows(2).all(|w| w[0].sequence < w[1].sequence));
    let raw = serde_json::to_string(&events).unwrap();
    assert!(raw.contains("WYSS_FIXTURE_OUTPUT") && !raw.contains("WYSS_FIXTURE_COMMAND"));
    assert!(
        events
            .iter()
            .filter(|e| e.managed_approval_id.is_some())
            .all(|e| e.managed_approval_id.as_ref() == Some(&p.shown.approval.managed_approval_id))
    );
    p.assert_private(&["WYSS_FIXTURE", "printf"]);
    p.finish();
}
#[test]
fn approval_protocol_presents_and_applies_one_file_change_exactly_once() {
    let mut p = pending("approval_file", local_scopes());
    let target = PathBuf::from(&p.workspace).join("wyss-fixture-probe.txt");
    assert_eq!(p.shown.approval.kind, ApprovalKind::FileChange);
    assert_eq!(p.shown.file_count, Some(1));
    let file = p.shown.file.as_ref().unwrap();
    assert_eq!(file.path, target.to_str().unwrap());
    assert_eq!(file.kind, FileChangeKind::Add);
    assert_eq!(
        (file.chunk.as_str(), file.next_offset, file.diff_bytes),
        ("+WYSS_FIXTURE_PATCH\n", None, 20)
    );
    // The complete proposal is visible and nothing was written while it is pending.
    thread::sleep(Duration::from_millis(300));
    assert!(!target.exists());
    assert_eq!(effects(&p.env, &p.workspace), 0);
    let accept = p.intent(ApprovalDecision::Accept);
    let receipt = p.client.approval_respond(accept.clone()).unwrap();
    assert_eq!(receipt.kind, ApprovalKind::FileChange);
    assert_eq!(
        wait_turn(&mut p.client, &p.turn, true).state,
        TurnState::Completed
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "WYSS_FIXTURE_PATCH\n");
    assert_eq!(lines(&p.env, "mutations"), 1);
    assert_eq!(p.client.approval_respond(accept), Ok(receipt));
    assert_eq!(decisions(&p.env), ["\"accept\""]);
    assert_eq!(lines(&p.env, "mutations"), 1);
    assert_eq!(p.status().approval.effect, ApprovalEffect::Confirmed);
    let lifecycle: Vec<_> = p
        .stream()
        .iter()
        .map(|e| e.kind)
        .filter(|k| !matches!(k, StreamKind::AssistantMessageDelta))
        .collect();
    assert_eq!(
        lifecycle,
        [
            StreamKind::TurnStarted,
            StreamKind::FileChangeProposed,
            StreamKind::ApprovalRequired,
            StreamKind::ApprovalResolved,
            StreamKind::FileChangeStarted,
            StreamKind::FileChangeCompleted,
            StreamKind::TurnCompleted,
        ]
    );
    // Neither the diff nor the path of the change is durable or streamed.
    let raw = serde_json::to_string(&p.stream()).unwrap();
    assert!(!raw.contains("WYSS_FIXTURE_PATCH") && !raw.contains("wyss-fixture-probe"));
    p.assert_private(&["WYSS_FIXTURE", "wyss-fixture-probe"]);
    p.finish();
}
#[test]
fn approval_protocol_refusals_map_to_the_provider_and_execute_nothing() {
    use ApprovalDecision::{Cancel, Decline};
    for (scenario, decision, answer, state, turn) in [
        (
            "approval_command",
            Decline,
            "\"cancel\"",
            ApprovalState::Declined,
            TurnState::Interrupted,
        ),
        (
            "approval_command",
            Cancel,
            "\"cancel\"",
            ApprovalState::Cancelled,
            TurnState::Interrupted,
        ),
        (
            "approval_file",
            Decline,
            "\"decline\"",
            ApprovalState::Declined,
            TurnState::Completed,
        ),
        (
            "approval_file",
            Cancel,
            "\"cancel\"",
            ApprovalState::Cancelled,
            TurnState::Interrupted,
        ),
    ] {
        let mut p = pending(scenario, local_scopes());
        let label = format!("{scenario}/{decision:?}");
        let before = p.env.methods().len();
        let intent = p.intent(decision);
        let receipt = p.client.approval_respond(intent.clone()).unwrap();
        assert_eq!(
            (receipt.decision, receipt.state),
            (decision, state),
            "{label}"
        );
        let done = wait_turn(&mut p.client, &p.turn, true);
        assert_eq!(done.state, turn, "{label}");
        if turn == TurnState::Interrupted {
            assert_eq!(
                done.terminal_reason,
                Some(TurnReason::UserCancel),
                "{label}"
            );
        }
        // A refusal needs no attestation: the one frame written is the refusal itself.
        assert_eq!(p.env.methods()[before..], ["RESPONSE"], "{label}");
        assert_eq!(decisions(&p.env), [answer], "{label}");
        assert_eq!(p.client.approval_respond(intent), Ok(receipt), "{label}");
        assert_eq!(decisions(&p.env), [answer], "{label}");
        assert_eq!(count(&p.env, "turn/interrupt"), 0, "{label}");
        assert_eq!(effects(&p.env, &p.workspace), 0, "{label}");
        let settled = p.status();
        assert_eq!(
            (settled.approval.state, settled.approval.effect),
            (state, ApprovalEffect::None),
            "{label}"
        );
        // Accepting what was refused is impossible.
        let mut late = p.intent(ApprovalDecision::Accept);
        late.expected_revision = settled.approval.revision;
        assert_eq!(
            p.client.approval_respond(late),
            Err(ControlError::InvalidTransition),
            "{label}"
        );
        let resolved = p
            .stream()
            .into_iter()
            .find(|e| e.kind == StreamKind::ApprovalResolved)
            .unwrap();
        assert_eq!(
            resolved.text.as_deref(),
            Some(if decision == Decline {
                "declined"
            } else {
                "cancelled"
            })
        );
        p.finish();
    }
}
#[test]
fn approval_protocol_denies_accept_without_authority_and_writes_no_accept_frame() {
    // The ORIGINAL lease lacks the scope of the action.
    let scopes: Vec<_> = local_scopes()
        .into_iter()
        .filter(|s| *s != ExecutionScope::CommandExecute)
        .collect();
    let mut p = pending("approval_command", scopes);
    assert_eq!(
        p.client
            .approval_respond(p.intent(ApprovalDecision::Accept)),
        Err(ControlError::GateDenied)
    );
    assert!(decisions(&p.env).is_empty());
    assert_eq!(p.status().approval.state, ApprovalState::Pending);
    assert_eq!(
        p.client
            .gate_status(GateQuery {
                session_id: p.session.managed_session_id.clone(),
                action: ExecutionScope::CommandExecute,
            })
            .unwrap()
            .reason,
        GateReason::ScopeNotGranted
    );
    // Refusing it needs no scope.
    p.client
        .approval_respond(p.intent(ApprovalDecision::Cancel))
        .unwrap();
    wait_turn(&mut p.client, &p.turn, true);
    assert_eq!(decisions(&p.env), ["\"cancel\""]);
    assert_eq!(effects(&p.env, &p.workspace), 0);
    p.finish();

    // The original lease was revoked: a replacement with every scope transfers nothing.
    let mut p = pending("approval_command", local_scopes());
    p.client
        .lease_revoke(LeaseRevoke {
            intent_id: id(),
            lease_id: p.lease.lease_id.clone(),
            expected_revision: p.lease.revision,
        })
        .unwrap();
    p.client
        .lease_grant(LeaseGrant {
            intent_id: id(),
            session_id: p.session.managed_session_id.clone(),
            expected_session_revision: 1,
            scopes: local_scopes(),
            ttl_seconds: 300,
        })
        .unwrap();
    assert_eq!(
        p.client
            .approval_respond(p.intent(ApprovalDecision::Accept)),
        Err(ControlError::InvalidTransition)
    );
    let done = wait_turn(&mut p.client, &p.turn, true);
    assert_eq!(done.terminal_reason, Some(TurnReason::LeaseRevoked));
    assert_eq!(decisions(&p.env), ["\"cancel\""]);
    assert_eq!(effects(&p.env, &p.workspace), 0);
    assert_eq!(
        (
            p.status().approval.state,
            p.status().approval.terminal_reason
        ),
        (
            ApprovalState::Invalidated,
            Some(ApprovalReason::LeaseRevoked)
        )
    );
    p.finish();

    // Safety drift between the request and the Accept: the profile cannot be re-proven.
    for drift in ["hooks", "project_policy"] {
        let mut p = pending("approval_command", local_scopes());
        if drift == "hooks" {
            fs::write(p.env.root.join("restore-hooks"), "").unwrap();
        } else {
            let layer = PathBuf::from(&p.workspace).join(".codex");
            fs::create_dir(&layer).unwrap();
            fs::write(layer.join("config.toml"), "approval_policy = \"never\"\n").unwrap();
        }
        assert_eq!(
            p.client
                .approval_respond(p.intent(ApprovalDecision::Accept)),
            Err(ControlError::GateDenied),
            "{drift}"
        );
        let failed = p.env.wait(&mut p.client, ActualState::Failed);
        assert_eq!(
            failed.diagnostic,
            Some(RuntimeDiagnostic::SafetyUnproven),
            "{drift}"
        );
        let turn = wait_turn(&mut p.client, &p.turn, true);
        assert_eq!(turn.state, TurnState::Indeterminate, "{drift}");
        let lost = p.status().approval;
        assert_eq!(
            (lost.state, lost.terminal_reason, lost.decision),
            (
                ApprovalState::Invalidated,
                Some(ApprovalReason::RuntimeLost),
                None
            ),
            "{drift}"
        );
        // No response of any kind was written, and nothing ran.
        assert!(decisions(&p.env).is_empty(), "{drift}");
        assert_eq!(count(&p.env, "RESPONSE"), 0, "{drift}");
        assert_eq!(effects(&p.env, &p.workspace), 0, "{drift}");
        p.finish();
    }
}
#[test]
fn accepted_answer_with_an_unknown_outcome_is_indeterminate_and_never_sent_or_run_again() {
    nix::sys::prctl::set_child_subreaper(true).unwrap();
    // The runtime dies after the Accept was written and before it reported any outcome.
    let mut p = pending("approval_command_slow", local_scopes());
    let accept = p.intent(ApprovalDecision::Accept);
    let receipt = p.client.approval_respond(accept.clone()).unwrap();
    wait_file(&p.env, "executions");
    let provider = *p.env.pids().last().unwrap();
    kill(Pid::from_raw(provider), Signal::SIGKILL).unwrap();
    p.env.wait(&mut p.client, ActualState::Failed);
    let turn = wait_turn(&mut p.client, &p.turn, true);
    assert_eq!(turn.state, TurnState::Indeterminate);
    let unknown = p.status().approval;
    assert_eq!(
        (unknown.state, unknown.effect, unknown.decision),
        (
            ApprovalState::Indeterminate,
            ApprovalEffect::Unknown,
            Some(ApprovalDecision::Accept)
        )
    );
    // The original receipt is still the answer to the same intent; nothing is resent.
    assert_eq!(
        p.client.approval_respond(accept.clone()),
        Ok(receipt.clone())
    );
    assert_eq!(decisions(&p.env), ["\"accept\""]);
    assert_eq!(lines(&p.env, "executions"), 1);
    // An authority restart and a new generation replay neither the answer nor the action.
    drop(p.client);
    p.authority.stop(false);
    p.authority = p.env.start();
    p.client = p.env.client();
    assert_eq!(p.status().approval, unknown);
    assert_eq!(p.client.approval_respond(accept), Ok(receipt));
    p.client.runtime_stop(mutation()).unwrap();
    p.env.wait(&mut p.client, ActualState::Stopped);
    p.client
        .runtime_start(local_mutation(&p.workspace))
        .unwrap();
    p.env.wait(&mut p.client, ActualState::Ready);
    assert_eq!(decisions(&p.env), ["\"accept\""]);
    assert_eq!(lines(&p.env, "executions"), 1);
    assert_eq!(count(&p.env, "turn/start"), 1);
    assert_eq!(
        p.client
            .binding_status(p.session.managed_session_id.clone())
            .unwrap()
            .state,
        BindingState::BoundDetached
    );
    p.finish();

    // The provider is gone between the durable dispatch marker and the response frame.
    let mut p = pending("approval_command_deaf", local_scopes());
    let accept = p.intent(ApprovalDecision::Accept);
    let receipt = p.client.approval_respond(accept.clone()).unwrap();
    assert_eq!(receipt.state, ApprovalState::Accepted);
    p.env.wait(&mut p.client, ActualState::Failed);
    let turn = wait_turn(&mut p.client, &p.turn, true);
    assert_eq!(turn.state, TurnState::Indeterminate);
    let unknown = p.status().approval;
    assert_eq!(
        (unknown.state, unknown.effect),
        (ApprovalState::Indeterminate, ApprovalEffect::Unknown)
    );
    assert_eq!(p.client.approval_respond(accept), Ok(receipt));
    // The provider never read an answer and nothing ran; the authority wrote at most one.
    assert!(decisions(&p.env).is_empty());
    assert_eq!(effects(&p.env, &p.workspace), 0);
    assert!(
        p.client
            .runtime_status()
            .unwrap()
            .outbound_methods
            .approval_accept
            <= 1
    );
    p.finish();
}

// Durable replay across the provider boundary: observation only, and content free.

/// The complete durable history through the public replay contract, in pages of `limit`.
fn replay(c: &mut ControlClient, limit: u16) -> Vec<serde_json::Value> {
    let stream_id = c.hello_status().stream_id.clone();
    let (mut all, mut after) = (Vec::new(), None);
    loop {
        let ResultValue::Audit { events, next } = c
            .audit(AuditRead {
                stream_id: stream_id.clone(),
                after_sequence: after,
                limit,
            })
            .unwrap()
        else {
            panic!("audit expected")
        };
        for e in &events {
            let v = serde_json::to_value(e).unwrap();
            assert_eq!(v["sequence"], (all.len() + 1).to_string(), "contiguous");
            all.push(v);
        }
        if next.is_none() {
            return all;
        }
        after = next;
    }
}
/// Every row of every table of the live database, read without the authority.
fn canonical(env: &Sandbox) -> String {
    let c = rusqlite::Connection::open_with_flags(
        env.root.join("state/wyss-console/control.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let tables: Vec<String> = c
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut out = String::new();
    for table in tables {
        let mut q = c
            .prepare(&format!("SELECT * FROM {table} ORDER BY 1"))
            .unwrap();
        let columns = q.column_count();
        let mut rows = q.query([]).unwrap();
        while let Some(r) = rows.next().unwrap() {
            out.push_str(&table);
            for i in 0..columns {
                out.push_str(&format!("|{:?}", r.get_ref(i).unwrap()));
            }
            out.push('\n');
        }
    }
    out
}
/// Everything a new provider frame, command or file write would have to change.
fn footprint(env: &Sandbox, c: &mut ControlClient, workspace: &str) -> serde_json::Value {
    let mut files: Vec<String> = fs::read_dir(workspace)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    serde_json::json!({
        "frames_written_to_the_provider": env.methods(),
        "answers": decisions(env),
        "executions": lines(env, "executions"),
        "mutations": lines(env, "mutations"),
        "workspace": files,
        "outbound": c.runtime_status().unwrap().outbound_methods,
        "processes": env.pids(),
    })
}
/// Replays the whole history every way the contract allows and proves nothing moved.
fn replay_changes_nothing(env: &Sandbox, c: &mut ControlClient, workspace: &str) -> usize {
    let stream_id = c.hello_status().stream_id.clone();
    let before = (footprint(env, c, workspace), canonical(env));
    let status = serde_json::to_value(c.status().unwrap()).unwrap();
    let whole = replay(c, 50);
    for limit in [1, 7, 50] {
        assert_eq!(replay(c, limit), whole, "limit {limit}");
        assert_eq!(
            replay(&mut env.client(), limit),
            whole,
            "another connection"
        );
    }
    let head = whole.len() as u64;
    for (after, wait_ms) in [(None, 0), (seq(head - 1), 5_000), (seq(head), 250)] {
        let ResultValue::Audit { events, .. } = c
            .audit_watch(AuditWatch {
                stream_id: stream_id.clone(),
                after_sequence: after,
                limit: 50,
                wait_ms,
            })
            .unwrap()
        else {
            panic!("audit expected")
        };
        assert_eq!(
            events.is_empty(),
            wait_ms == 250,
            "caught up only at the head"
        );
    }
    assert_eq!(serde_json::to_value(c.status().unwrap()).unwrap(), status);
    assert_eq!((footprint(env, c, workspace), canonical(env)), before);
    whole.len()
}
fn seq(n: u64) -> Option<wyss_console::contracts::Sequence> {
    (n > 0).then(|| n.to_string().parse().unwrap())
}
fn kinds(history: &[serde_json::Value], family: &str) -> Vec<String> {
    history
        .iter()
        .filter_map(|e| e["event_type"].as_str()?.strip_prefix(family))
        .map(str::to_owned)
        .collect()
}
fn wait_pending(c: &mut ControlClient, t: &ManagedTurnRecord) -> ApprovalStatus {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(shown) = c.approval_pending(t.managed_turn_id.clone()).unwrap() {
            return shown;
        }
        assert!(Instant::now() < deadline, "no pending approval");
        thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn replaying_accepted_cancelled_and_revoked_history_causes_no_effect_anywhere() {
    nix::sys::prctl::set_child_subreaper(true).unwrap();
    // One history: an accepted command, a cancelled turn and a revoked lease.
    let mut p = pending("approval_command", local_scopes());
    let accept = p.intent(ApprovalDecision::Accept);
    p.client.approval_respond(accept.clone()).unwrap();
    assert_eq!(
        wait_turn(&mut p.client, &p.turn, true).state,
        TurnState::Completed
    );
    let cancelled = p.client.turn_start(turn_params(&p.session)).unwrap();
    wait_pending(&mut p.client, &cancelled);
    let running = wait_turn(&mut p.client, &cancelled, false);
    p.client
        .turn_cancel(TurnCancel {
            intent_id: id(),
            managed_turn_id: cancelled.managed_turn_id.clone(),
            expected_revision: running.revision,
        })
        .unwrap();
    assert_eq!(
        wait_turn(&mut p.client, &cancelled, true).state,
        TurnState::Interrupted
    );
    let revoked = p.client.turn_start(turn_params(&p.session)).unwrap();
    wait_pending(&mut p.client, &revoked);
    p.client
        .lease_revoke(LeaseRevoke {
            intent_id: id(),
            lease_id: p.lease.lease_id.clone(),
            expected_revision: p.lease.revision,
        })
        .unwrap();
    assert_eq!(
        wait_turn(&mut p.client, &revoked, true).state,
        TurnState::Interrupted
    );
    assert_eq!(lines(&p.env, "executions"), 1);
    assert_eq!(
        decisions(&p.env).first().map(String::as_str),
        Some("\"accept\"")
    );
    let accepts = |env: &Sandbox| decisions(env).iter().filter(|d| *d == "\"accept\"").count();
    assert_eq!(accepts(&p.env), 1);

    let events = replay_changes_nothing(&p.env, &mut p.client, &p.workspace);
    let history = replay(&mut p.client, 50);
    assert_eq!(history.len(), events);
    assert_eq!(
        kinds(&history, "authority.managed_approval."),
        [
            "requested",
            "accepted",
            "confirmed",
            "requested",
            "invalidated",
            "requested",
            "invalidated"
        ]
    );
    assert_eq!(
        kinds(&history, "authority.execution_lease."),
        ["granted", "revoked"]
    );
    assert!(kinds(&history, "authority.managed_turn.").contains(&"cancel_requested".to_owned()));
    // Canonical state is what the tables say, and a decided intent retried after the
    // replay still returns its receipt without a second frame or a second execution.
    assert_eq!(
        p.client.lease_get(p.lease.lease_id.clone()).unwrap().state,
        LeaseState::Revoked
    );
    let answers = decisions(&p.env).len();
    assert!(p.client.approval_respond(accept).is_ok());
    assert_eq!(decisions(&p.env).len(), answers);
    assert_eq!(lines(&p.env, "executions"), 1);
    // After a restart the committed history is intact, and replaying it into the new
    // process starts no runtime work, resumes no thread and answers nothing.
    let (turns, resumes) = (count(&p.env, "turn/start"), count(&p.env, "thread/resume"));
    drop(p.client);
    p.authority.stop(false);
    let mut b = p.env.start();
    let mut c = p.env.client();
    p.env.wait(&mut c, ActualState::Ready);
    assert_eq!(replay(&mut c, 50)[..events], history[..]);
    replay_changes_nothing(&p.env, &mut c, &p.workspace);
    assert_eq!(
        (count(&p.env, "turn/start"), count(&p.env, "thread/resume")),
        (turns, resumes)
    );
    assert_eq!((accepts(&p.env), lines(&p.env, "executions")), (1, 1));
    assert_eq!(decisions(&p.env).len(), answers);
    drop(c);
    b.stop(false);
    for pid in p.env.pids() {
        dead(pid);
    }
}
#[test]
fn replaying_an_accepted_file_change_writes_nothing_again() {
    let mut p = pending("approval_file", local_scopes());
    let target = PathBuf::from(&p.workspace).join("wyss-fixture-probe.txt");
    p.client
        .approval_respond(p.intent(ApprovalDecision::Accept))
        .unwrap();
    assert_eq!(
        wait_turn(&mut p.client, &p.turn, true).state,
        TurnState::Completed
    );
    let written = fs::metadata(&target).unwrap();
    fs::remove_file(&target).unwrap();
    // Even with the file gone, replaying the history that accepted it never recreates it.
    let events = replay_changes_nothing(&p.env, &mut p.client, &p.workspace);
    let history = replay(&mut p.client, 50);
    assert_eq!(history.len(), events);
    assert_eq!(
        kinds(&history, "authority.managed_approval."),
        ["requested", "accepted", "confirmed"]
    );
    assert!(!target.exists());
    assert_eq!(lines(&p.env, "mutations"), 1);
    assert_eq!(decisions(&p.env), ["\"accept\""]);
    assert!(written.len() > 0);
    p.finish();
}
/// No sentinel in the replayed history or in any byte of any file the authority keeps,
/// database, write-ahead log and shared memory included, while it is running.
fn assert_content_free(env: &Sandbox, c: &mut ControlClient, sentinels: &[&str]) -> usize {
    let history = serde_json::to_string(&replay(c, 50)).unwrap();
    let mut scanned = 0;
    let mut pending = vec![env.root.join("state"), env.root.join("run")];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let kind = fs::symlink_metadata(&path).unwrap().file_type();
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() {
                let bytes = fs::read(&path).unwrap_or_default();
                scanned += 1;
                for s in sentinels {
                    assert!(
                        !bytes.windows(s.len()).any(|w| w == s.as_bytes()),
                        "{s} is durable in {}",
                        path.display()
                    );
                }
            }
        }
    }
    for s in sentinels {
        assert!(!history.contains(s), "{s} is replayed");
    }
    for name in [
        "control.sqlite3",
        "control.sqlite3-wal",
        "control.sqlite3-shm",
    ] {
        assert!(
            env.root.join("state/wyss-console").join(name).exists(),
            "{name} is live"
        );
    }
    scanned
}
#[test]
fn durable_replay_and_every_authority_file_hold_no_content() {
    // Inference: the prompt, the answer and both kinds of reasoning.
    let env = Sandbox::new("happy");
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(mutation()).unwrap();
    env.wait(&mut c, ActualState::Ready);
    let (s, _) = inference_session(&env, &mut c);
    let params = turn_params(&s);
    let t = c.turn_start(params.clone()).unwrap();
    assert_eq!(wait_turn(&mut c, &t, true).state, TurnState::Completed);
    let stream = |c: &mut ControlClient| {
        serde_json::to_string(
            &c.turn_events(TurnEvents {
                managed_turn_id: t.managed_turn_id.clone(),
                after_sequence: None,
                limit: 50,
            })
            .unwrap(),
        )
        .unwrap()
    };
    // The sentinels are real: the transient stream carries them while the process lives.
    assert!(stream(&mut c).contains("ASSISTANT_SENTINEL"));
    let content = [
        params.prompt.as_str(),
        "WYSS_2H_SECRET_PROMPT_",
        "ASSISTANT_SENTINEL",
        "SUMMARY_SENTINEL",
        "RAW_PRIVATE_REASONING_SENTINEL",
    ];
    assert!(assert_content_free(&env, &mut c, &content) >= 3);
    let before = replay(&mut c, 50);
    drop(c);
    a.stop(false);
    a = env.start();
    let mut c = env.client();
    env.wait(&mut c, ActualState::Ready);
    // Across a restart the durable history survives and the transient stream does not.
    assert_eq!(replay(&mut c, 50)[..before.len()], before[..]);
    assert!(!stream(&mut c).contains("ASSISTANT_SENTINEL"));
    assert_content_free(&env, &mut c, &content);
    c.runtime_stop(mutation()).unwrap();
    env.wait(&mut c, ActualState::Stopped);
    drop(c);
    a.stop(false);
    for pid in env.pids() {
        dead(pid);
    }
    // Local tools: the command, its output, the file path, its content and the diff, and
    // the provider credential the isolated runtime can read.
    nix::sys::prctl::set_child_subreaper(true).unwrap();
    for (scenario, action) in [
        ("approval_command", vec!["WYSS_FIXTURE_COMMAND", "printf "]),
        (
            "approval_file",
            vec!["WYSS_FIXTURE_PATCH", "wyss-fixture-probe"],
        ),
    ] {
        let mut p = pending(scenario, local_scopes());
        p.client
            .approval_respond(p.intent(ApprovalDecision::Accept))
            .unwrap();
        assert_eq!(
            wait_turn(&mut p.client, &p.turn, true).state,
            TurnState::Completed
        );
        let shown = serde_json::to_string(&p.shown).unwrap();
        let streamed = serde_json::to_string(&p.stream()).unwrap();
        assert!(
            shown.contains(action[0]),
            "the presentation is transient and real"
        );
        let mut content = action.clone();
        content.extend([
            "WYSS_2H_SECRET_PROMPT_",
            "FIXTURE-CREDENTIAL",
            "ASSISTANT_SENTINEL",
        ]);
        assert_content_free(&p.env, &mut p.client, &content);
        // What replay does carry is the fingerprint of the action and nothing of it.
        let history = serde_json::to_string(&replay(&mut p.client, 50)).unwrap();
        assert!(history.contains(&p.shown.approval.action_fingerprint));
        // The transient stream announces the action and carries none of it either.
        assert!(!streamed.contains(action[0]) || scenario == "approval_command");
        p.finish();
    }
}
