//! M1-12 against a real host process: a separate `--wyss-host` serving real terminals on a
//! socket in a directory made for the test (its own runtime, state and home). The owner of the
//! tester is never named; the host of the user is never touched. The process is killed by the
//! exact handle that started it.
use super::*;
use crate::projects::{
    claim::Claims,
    owner::{HostSocket, NoManaged, RealOwner},
    service::ProjectsService,
};
use std::{
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

/// Run as a disposable child. Alone it does nothing; with the variable set it is the host.
#[test]
fn child_real_host() {
    if std::env::var_os("WYSS_M1_REAL_HOST").is_none() {
        return;
    }
    // `host::run` with a plain shell standing in for the user's, which would print a greeting
    // and wait for a terminal query. Everything else is the host as it runs.
    use crate::host::{core::HostCore, server};
    let path = server::socket_path().expect("a runtime directory");
    let Ok(listener) = server::bind(&path) else {
        std::process::exit(2);
    };
    let core = HostCore::new(crate::native::open_from_environment());
    core.use_shell(PathBuf::from("/bin/sh"));
    server::run(listener, core, server::IDLE_EXIT);
    std::process::exit(0);
}

struct Real {
    runtime: PathBuf,
    child: Option<Child>,
}

impl Real {
    fn socket(root: &std::path::Path) -> PathBuf {
        root.join("run").join("wyss-console").join("host.sock")
    }

    fn start(root: &std::path::Path) -> Self {
        let run = root.join("run");
        crate::native::store::private_dir(&run).unwrap();
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "projects::tests::process_tests::child_real_host",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("WYSS_M1_REAL_HOST", "1")
            .env("XDG_RUNTIME_DIR", &run)
            .env("XDG_STATE_HOME", root.join("hoststate"))
            .env("HOME", &home)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("a disposable host process");
        let this = Self {
            runtime: run,
            child: Some(child),
        };
        let socket = Self::socket(root);
        let deadline = Instant::now() + Duration::from_secs(10);
        while HostSocket::new(socket.clone()).epoch().is_err() {
            assert!(
                Instant::now() < deadline,
                "the disposable host did not come up"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        this
    }

    fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_file(self.runtime.join("wyss-console").join("host.sock"));
    }
}

impl Drop for Real {
    fn drop(&mut self) {
        self.kill();
    }
}

fn service(root: &std::path::Path, env: Arc<FakeEnv>) -> ProjectsService {
    let owner = RealOwner::new(Some(HostSocket::new(Real::socket(root))), NoManaged);
    ProjectsService::new(
        Some(root.join("state/wyss-console/projects/v1")),
        Arc::new(owner),
        env,
        Claims::new(Some(root.join("claims"))),
    )
    .with_inventory_ttl(Duration::ZERO)
}

fn until<T>(mut poll: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(found) = poll() {
            return found;
        }
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn a_real_terminal_is_launched_attached_read_and_typed_into_through_the_checked_socket() {
    let dir = TestDir::new();
    let mut host = Real::start(&dir.0);
    let project_dir = dir.0.join("project");
    std::fs::create_dir_all(&project_dir).unwrap();
    let env = Arc::new(FakeEnv::new());
    let service = service(&dir.0, env.clone());
    service.set_enabled(true).unwrap();
    service.initialize_empty().unwrap();
    let run = |ctx: Value, command: Value| {
        let status = service.overview();
        let revision = match status.store {
            crate::projects::service::StoreStatus::Ready { revision, .. } => revision,
            other => panic!("{other:?}"),
        };
        service.execute(
            serde_json::from_value(json!({
                "schema": 1, "operationId": env.uuid(), "expectedStoreRevision": revision,
                "context": ctx, "command": command,
            }))
            .unwrap(),
        )
    };
    let status = service.overview();
    let (host_id, scratch) = match status.store {
        crate::projects::service::StoreStatus::Ready {
            host_id,
            scratch_workspace_id,
            ..
        } => (host_id, scratch_workspace_id),
        other => panic!("{other:?}"),
    };
    assert!(status.host.reachable && status.host.epoch.is_some());
    let epoch_before = status.host.epoch.clone().unwrap();
    let ctx = json!({"hostId": host_id, "projectId": null, "workspaceId": scratch, "bindingId": null,
                     "bindingRevision": null, "selectionRevision": "1"});
    // A pane in Scratch, and a plain shell started in it.
    let leaf = env.uuid();
    run(ctx.clone(), json!({"kind": "saveWorkspace", "workspaceId": scratch, "expectedWorkspaceRevision": "1",
        "label": "Scratch", "layout": {"kind": "leaf", "id": leaf}, "focus": null, "pinned": false,
        "leaves": [leaf_json(&leaf, Value::Null)],
        "preferences": {"provider": null, "mode": "native", "nativeProfile": null, "managedSafetyProfile": null}}))
        .unwrap();
    let launched = run(ctx.clone(), json!({"kind": "launch", "workspaceId": scratch, "leafId": leaf,
        "provider": "shell", "mode": "native", "nativeProfile": null, "managedSafetyProfile": null}))
        .unwrap();
    let association = launched.created[0].id.clone();
    let view = service.get_workspace(&scratch, Some("w1")).unwrap();
    let shown = view.leaves[0].session.clone().unwrap();
    assert_eq!((shown.kind, shown.availability), ("shell", "live"));

    // Input is exclusive; a mirror reads. The shell answers what is typed.
    service.claim_input("w1", &association).unwrap();
    assert!(service.claim_input("w2", &association).is_err());
    assert!(service.write_input("w2", &association, b"x").is_err());
    // The typed text echoes back; only a shell that ran it prints the second word joined.
    service
        .write_input("w1", &association, b"printf 'm1-%s-end\\n' marker\n")
        .unwrap();
    let mut from = 0;
    let mut seen = Vec::new();
    until(|| {
        let out = service.read_output(&association, from, 200).ok()?;
        from = out.next;
        seen.extend(out.data);
        String::from_utf8_lossy(&seen)
            .contains("m1-marker-end")
            .then_some(())
    });
    service.resize("w1", &association, 30, 100).unwrap();

    // The host is replaced. Nothing is started to replace it, nothing is guessed: the session is
    // reported as belonging to a host that is gone, and every call is refused before it is sent.
    host.kill();
    assert_eq!(
        service.read_output(&association, 0, 0).unwrap_err().code,
        ErrorCode::HostOffline
    );
    let offline = service.get_workspace(&scratch, Some("w1")).unwrap().leaves[0]
        .session
        .clone()
        .unwrap();
    assert_eq!(
        (offline.availability, offline.reason),
        ("unknown", Some("host_offline"))
    );
    assert!(
        !HostSocket::new(Real::socket(&dir.0)).epoch().is_ok(),
        "reading never starts a host"
    );
    let _second = Real::start(&dir.0);
    let after = service.overview();
    assert!(after.host.reachable);
    assert_ne!(
        after.host.epoch.as_deref(),
        Some(epoch_before.as_str()),
        "a new process is a new epoch"
    );
    let ended = service.get_workspace(&scratch, Some("w1")).unwrap().leaves[0]
        .session
        .clone()
        .unwrap();
    assert_eq!(
        (ended.availability, ended.reason),
        ("ended", Some("host_changed"))
    );
    assert_eq!(
        service.read_output(&association, 0, 0).unwrap_err().code,
        ErrorCode::HostChanged
    );
    assert_eq!(
        service
            .write_input("w1", &association, b"echo no\n")
            .unwrap_err()
            .code,
        ErrorCode::HostChanged
    );
    // Attach is refused, and the new host holds no terminal that anything here created.
    let attach = run(
        ctx.clone(),
        json!({"kind": "attach", "workspaceId": scratch, "leafId": leaf, "associationId": association}),
    );
    assert_eq!(attach.unwrap_err().code, ErrorCode::HostChanged);
    let rows = crate::projects::owner::HostSocket::new(Real::socket(&dir.0))
        .host_view(None)
        .unwrap()
        .terminals;
    assert!(rows.is_empty(), "no shell was recreated: {rows:?}");
}

#[test]
fn a_real_host_gives_the_same_epoch_to_every_read_and_a_project_launch_runs_in_its_folder() {
    let dir = TestDir::new();
    let _host = Real::start(&dir.0);
    let project_dir = std::fs::canonicalize(&dir.0).unwrap().join("work");
    std::fs::create_dir_all(&project_dir).unwrap();
    let env = Arc::new(FakeEnv::new());
    let service = service(&dir.0, env.clone());
    service.set_enabled(true).unwrap();
    service.initialize_empty().unwrap();
    let e1 = service.overview().host.epoch;
    let e2 = service.overview().host.epoch;
    assert_eq!(e1, e2);
    let pick = service
        .issue_pick(project_dir.to_str().unwrap(), Purpose::Register, 1)
        .unwrap();
    let status = service.overview();
    let (host_id, revision) = match status.store {
        crate::projects::service::StoreStatus::Ready {
            host_id, revision, ..
        } => (host_id, revision),
        other => panic!("{other:?}"),
    };
    let ctx = json!({"hostId": host_id, "projectId": null, "workspaceId": null, "bindingId": null,
                     "bindingRevision": null, "selectionRevision": "1"});
    let receipt = service
        .execute(
            serde_json::from_value(json!({"schema": 1, "operationId": env.uuid(),
            "expectedStoreRevision": revision, "context": ctx,
            "command": {"kind": "registerExisting", "selectionToken": pick.token}}))
            .unwrap(),
        )
        .unwrap();
    let id = |role: &str| {
        receipt
            .created
            .iter()
            .find(|c| c.role == role)
            .unwrap()
            .id
            .clone()
    };
    let (project, workspace) = (id("project"), id("workspace"));
    let leaf = env.uuid();
    let store_view = service.get_project(&project).unwrap();
    let root = store_view.root.unwrap();
    let context = |ws: &str| {
        json!({"hostId": host_id, "projectId": project, "workspaceId": ws,
        "bindingId": root.binding_id, "bindingRevision": root.binding_revision, "selectionRevision": "1"})
    };
    let revision_now = |service: &ProjectsService| match service.overview().store {
        crate::projects::service::StoreStatus::Ready { revision, .. } => revision,
        other => panic!("{other:?}"),
    };
    service
        .execute(serde_json::from_value(json!({"schema": 1, "operationId": env.uuid(),
            "expectedStoreRevision": revision_now(&service), "context": context(&workspace),
            "command": {"kind": "saveWorkspace", "workspaceId": workspace, "expectedWorkspaceRevision": "1",
                "label": "Main", "layout": {"kind": "leaf", "id": leaf}, "focus": null, "pinned": false,
                "leaves": [leaf_json(&leaf, Value::Null)],
                "preferences": {"provider": null, "mode": "native", "nativeProfile": null, "managedSafetyProfile": null}}})).unwrap())
        .unwrap();
    let launched = service
        .execute(serde_json::from_value(json!({"schema": 1, "operationId": env.uuid(),
            "expectedStoreRevision": revision_now(&service), "context": context(&workspace),
            "command": {"kind": "launch", "workspaceId": workspace, "leafId": leaf, "provider": "shell",
                "mode": "native", "nativeProfile": null, "managedSafetyProfile": null}})).unwrap())
        .unwrap();
    let association = launched.created[0].id.clone();
    service.claim_input("w1", &association).unwrap();
    service.write_input("w1", &association, b"pwd\n").unwrap();
    let mut from = 0;
    let mut seen = Vec::new();
    let expected = project_dir.to_str().unwrap().to_owned();
    until(|| {
        let out = service.read_output(&association, from, 200).ok()?;
        from = out.next;
        seen.extend(out.data);
        String::from_utf8_lossy(&seen)
            .contains(&expected)
            .then_some(())
    });
}
