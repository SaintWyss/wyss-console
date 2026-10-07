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
    assert_eq!(env.pids().len(), 1);
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
    assert_eq!(env.methods(), ["initialize", "initialized", "model/list"]);
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
    dead(env.pids()[0]);
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
    dead(env.pids()[1]);
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
    dead(env.pids()[2]);
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
    assert_eq!(env.pids().len(), 3);
    d.stop(false);
    let mut e = env.start();
    let mut c = env.client();
    let final_state = c.runtime_status().unwrap();
    assert_eq!(final_state.actual_state, ActualState::Stopped);
    assert_eq!(final_state.desired_state, DesiredState::Stopped);
    assert_eq!(final_state.revision, 2);
    assert_eq!(env.pids().len(), 3);
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
        dead(env.pids()[0]);
        assert_eq!(env.pids().len(), 1);
        assert_eq!(env.methods(), ["initialize", "initialized", "model/list"]);
        if scenario == "server_request" {
            assert_eq!(fs::read_to_string(env.root.join("responses")).unwrap(), "0");
            assert_eq!(failed.diagnostic, Some(RuntimeDiagnostic::ServerRequest));
        }
        for _ in 0..3 {
            assert_eq!(
                c.runtime_status().unwrap().actual_state,
                ActualState::Failed
            );
        }
        assert_eq!(env.pids().len(), 1);
        // New explicit intent admits exactly one new attempt; an ambiguous retry does not.
        let retry = mutation();
        let r = c.runtime_start(retry.clone()).unwrap();
        assert_eq!(r.revision, 2);
        env.wait(&mut c, ActualState::Failed);
        assert_eq!(c.runtime_start(retry).unwrap(), r);
        assert_eq!(env.pids().len(), 2);
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
    assert_eq!(env.methods(), ["initialize", "initialized", "model/list"]);
    a.stop(false);
    dead(env.pids()[0]);
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
    assert_eq!(failed.outbound_methods.initialize, 1);
    assert_eq!(failed.outbound_methods.initialized, 1);
    assert_eq!(failed.outbound_methods.model_list, 1);
    dead(env.pids()[0]);
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
    assert_eq!(status.outbound_methods.initialize, 1);
    assert_eq!(status.outbound_methods.initialized, 1);
    assert_eq!(status.outbound_methods.model_list, 1);
    let methods = env.methods();
    assert_eq!(methods, ["initialize", "initialized", "model/list"]);
    let previous = status.runtime_instance_id;
    assert_eq!(c.runtime_start(mutation()).unwrap().revision, 2);
    assert_eq!(c.runtime_status().unwrap().runtime_instance_id, previous);
    assert_eq!(env.pids().len(), 1);
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
