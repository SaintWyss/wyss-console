//! The protocol-2 authority as the real `wyss-control --protocol 2` process, in isolated
//! temporary directories, reached the way the window reaches it: by its private socket.
//! No provider, no credential and no installed path is involved.
#![cfg(target_os = "linux")]
use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, FileTypeExt, PermissionsExt},
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use wyss_console::{
    contracts::WireUuid,
    f8::gui::{F8Failure, F8Gui, SocketConnector},
    sessions::AgentMode,
};

fn id() -> WireUuid {
    WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap()
}

struct Sandbox {
    root: PathBuf,
}
impl Sandbox {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("wyss-f8-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        for d in ["run", "state"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(d))
                .unwrap();
        }
        Self { root }
    }
    fn run(&self) -> PathBuf {
        self.root.join("run")
    }
    fn state(&self) -> PathBuf {
        self.root.join("state")
    }
    fn socket(&self) -> PathBuf {
        self.run().join("wyss-console/control.sock")
    }
    fn image(&self) -> PathBuf {
        self.state().join("wyss-console/authority.sqlite3")
    }
    fn command(&self, extra: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_wyss-control"));
        command
            .args(["--protocol", "2", "--runtime-root"])
            .arg(self.run())
            .arg("--state-root")
            .arg(self.state())
            .args(extra)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }
    fn start(&self, extra: &[&str]) -> Authority {
        Authority {
            child: self.command(extra).spawn().unwrap(),
        }
    }
    fn gui(&self) -> F8Gui {
        F8Gui::local(SocketConnector::new(self.run()))
    }
    /// A window service whose first greeting succeeds: the process has bound its socket.
    fn connected(&self) -> F8Gui {
        let gui = self.gui();
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            if gui.reconnect().authority.is_some() {
                return gui;
            }
            assert!(Instant::now() < end, "the authority never answered");
            thread::sleep(Duration::from_millis(25));
        }
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
struct Authority {
    child: Child,
}
impl Authority {
    fn stop(&mut self) -> std::process::ExitStatus {
        kill(Pid::from_raw(self.child.id() as i32), Signal::SIGTERM).unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < end, "the authority did not stop");
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn exit(&mut self) -> i32 {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.code().unwrap();
            }
            assert!(Instant::now() < end, "the authority did not exit");
            thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Authority {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn it_serves_protocol_2_on_a_private_socket_and_stops_on_a_signal() {
    let sandbox = Sandbox::new();
    let mut authority = sandbox.start(&["--create"]);
    let gui = sandbox.connected();
    let state = gui.state();
    assert!(state.active);
    let status = state.authority.unwrap();
    assert_eq!(status.protocol_version, 2);
    assert_eq!(status.storage_schema_version, 8);
    let socket = fs::symlink_metadata(sandbox.socket()).unwrap();
    assert!(socket.file_type().is_socket());
    assert_eq!(socket.permissions().mode() & 0o777, 0o600);
    assert_eq!(
        fs::metadata(sandbox.image()).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let status = authority.stop();
    assert!(status.success(), "{status:?}");
    assert!(
        !sandbox.socket().exists(),
        "the socket is removed on a clean stop"
    );
}

#[test]
fn a_session_survives_a_restart_and_the_image_is_not_created_twice() {
    let sandbox = Sandbox::new();
    let mut first = sandbox.start(&["--create"]);
    let gui = sandbox.connected();
    let session = gui.prepare(id()).unwrap().id;
    let instance = gui.state().authority.unwrap().instance_id;
    assert!(first.stop().success());
    drop(gui);

    // `--create` over an image is refused, and says so before touching it.
    let mut again = sandbox.start(&["--create"]);
    assert_eq!(again.exit(), 2);

    let mut second = sandbox.start(&[]);
    let gui = sandbox.connected();
    let after = gui.state().authority.unwrap();
    assert_ne!(after.instance_id, instance, "a new run is a new instance");
    let sessions = gui.sessions(None).unwrap();
    assert_eq!(sessions.sessions.len(), 1);
    assert_eq!(sessions.sessions[0].id, session);
    assert!(second.stop().success());
}

#[test]
fn without_a_runtime_the_authority_prepares_sessions_and_refuses_to_start_one() {
    let sandbox = Sandbox::new();
    let mut authority = sandbox.start(&["--create"]);
    let gui = sandbox.connected();
    let id_of_session = gui.prepare(id()).unwrap().id;
    let row = gui.session(id_of_session.clone()).unwrap().session;
    let started = gui.runtime_start(
        id(),
        id_of_session,
        row.revision,
        row.runtime.generation,
        AgentMode::Inference,
        None,
    );
    let error = started.expect_err("no runtime exists in this authority");
    assert_ne!(error.code, F8Failure::Inactive);
    // Nothing was reserved: the runtime is exactly as it was.
    let after = gui.sessions(None).unwrap();
    assert_eq!(after.sessions[0].runtime.generation, row.runtime.generation);
    assert!(authority.stop().success());
}

#[test]
fn it_never_opens_a_directory_that_holds_a_protocol_1_store() {
    let sandbox = Sandbox::new();
    let product = sandbox.state().join("wyss-console");
    fs::DirBuilder::new().mode(0o700).create(&product).unwrap();
    let store = product.join("control.sqlite3");
    fs::write(&store, b"not a database, and not touched").unwrap();
    fs::set_permissions(&store, fs::Permissions::from_mode(0o600)).unwrap();
    for extra in [&["--create"][..], &[][..]] {
        let mut authority = sandbox.start(extra);
        assert_eq!(authority.exit(), 2);
    }
    assert_eq!(
        fs::read(&store).unwrap(),
        b"not a database, and not touched"
    );
    assert!(!sandbox.image().exists());
    assert!(!sandbox.socket().exists());
}

#[test]
fn an_absent_image_is_not_created_unless_asked() {
    let sandbox = Sandbox::new();
    let mut authority = sandbox.start(&[]);
    assert_eq!(authority.exit(), 1);
    assert!(!sandbox.image().exists());
}

#[test]
fn a_second_authority_on_the_same_directories_is_refused() {
    let sandbox = Sandbox::new();
    let mut first = sandbox.start(&["--create"]);
    let gui = sandbox.connected();
    let mut second = sandbox.start(&[]);
    assert_eq!(second.exit(), 2);
    // The first one was not disturbed.
    assert!(gui.reconnect().authority.is_some());
    assert!(first.stop().success());
}

#[test]
fn it_refuses_any_shape_of_arguments_it_was_not_given_by_name() {
    let sandbox = Sandbox::new();
    let run = sandbox.run();
    let state = sandbox.state();
    let (run, state) = (run.to_str().unwrap(), state.to_str().unwrap());
    let cases: &[&[&str]] = &[
        &[
            "--protocol",
            "1",
            "--runtime-root",
            run,
            "--state-root",
            state,
        ],
        &["--protocol", "2", "--runtime-root", run],
        &["--protocol", "2", "--state-root", state],
        &["--runtime-root", run, "--state-root", state],
        &[
            "--protocol",
            "2",
            "--runtime-root",
            "run",
            "--state-root",
            state,
        ],
        &[
            "--protocol",
            "2",
            "--runtime-root",
            run,
            "--state-root",
            state,
            "--other",
        ],
        &[
            "--protocol",
            "2",
            "--runtime-root",
            run,
            "--state-root",
            state,
            "--create",
            "--create",
        ],
        &[
            "--protocol",
            "2",
            "--protocol",
            "2",
            "--runtime-root",
            run,
            "--state-root",
            state,
        ],
        &["extra"],
    ];
    for case in cases {
        let status = Command::new(env!("CARGO_BIN_EXE_wyss-control"))
            .args(*case)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(2), "{case:?}");
    }
    assert!(!sandbox.image().exists());
}

#[test]
fn a_rehearsal_migrates_a_copy_and_leaves_the_copy_alone() {
    let sandbox = Sandbox::new();
    // A protocol-1 store with one session, made by the real protocol-1 authority.
    let v1 = sandbox.root.join("v1");
    for d in ["", "run", "state"] {
        fs::DirBuilder::new()
            .mode(0o700)
            .recursive(true)
            .create(v1.join(d))
            .unwrap();
    }
    let mut old = Command::new(env!("CARGO_BIN_EXE_wyss-control"))
        .env_clear()
        .env("XDG_RUNTIME_DIR", v1.join("run"))
        .env("XDG_STATE_HOME", v1.join("state"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let client = {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            match wyss_console::control::client::ControlClient::connect_in_runtime(&v1.join("run"))
            {
                Ok(c) => break c,
                Err(e) => {
                    assert!(Instant::now() < end, "protocol 1 startup: {e}");
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
    };
    let mut client = client;
    client
        .prepare(wyss_console::contracts::control::Prepare {
            intent_id: id(),
            adapter_id: wyss_console::contracts::agent_adapter::AgentAdapterId::parse("codex")
                .unwrap(),
            runtime_kind: wyss_console::contracts::agent_adapter::AgentRuntimeKind::Codex,
        })
        .unwrap();
    drop(client);
    kill(Pid::from_raw(old.id() as i32), Signal::SIGTERM).unwrap();
    assert!(old.wait().unwrap().success());
    // The copy is what the rehearsal reads; the store itself is never named.
    let copy = sandbox.root.join("rehearsal.sqlite3");
    let store = v1.join("state/wyss-console/control.sqlite3");
    let made = Command::new("sqlite3")
        .arg(&store)
        .arg(format!(".backup '{}'", copy.display()))
        .status()
        .expect("the rehearsal needs the sqlite3 tool to make its copy");
    assert!(made.success());
    fs::set_permissions(&copy, fs::Permissions::from_mode(0o600)).unwrap();
    let before = fs::read(&copy).unwrap();
    // A copy that is not the caller's private file, or that does not exist, is refused, and
    // the installed store's own name is never accepted, whatever it is reached through.
    let loose = sandbox.root.join("loose.sqlite3");
    fs::copy(&copy, &loose).unwrap();
    fs::set_permissions(&loose, fs::Permissions::from_mode(0o644)).unwrap();
    let link = sandbox.root.join("link.sqlite3");
    std::os::unix::fs::symlink(&store, &link).unwrap();
    for refused in [
        loose,
        sandbox.root.join("absent.sqlite3"),
        link,
        store.clone(),
    ] {
        let mut attempt = sandbox.start(&["--migrate-copy", refused.to_str().unwrap()]);
        assert_eq!(attempt.exit(), 2, "{refused:?}");
        assert!(!sandbox.image().exists(), "{refused:?} left an image");
        assert!(
            !sandbox.root.join("absent.sqlite3").exists(),
            "it was created"
        );
    }
    let mut rehearsal = sandbox.start(&["--migrate-copy", copy.to_str().unwrap()]);
    let gui = sandbox.connected();
    let sessions = gui.sessions(None).unwrap();
    assert_eq!(sessions.sessions.len(), 1, "history crossed the migration");
    assert!(rehearsal.stop().success());
    assert_eq!(
        fs::read(&copy).unwrap(),
        before,
        "the copy is read, not changed"
    );
}
