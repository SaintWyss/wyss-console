//! The authority's lifecycle with the real things: the real `wyss-control`, started by a
//! real `systemd --user`, found by the real `AuthorityService`.
//!
//! The service manager is a private one. Each test starts its own `systemd --user` on a
//! temporary runtime directory and an empty temporary home, so the unit under test is the
//! one the product writes, unchanged, and nothing of the user's own session, units or
//! Wyss state is read or touched. The runtime is the repository's Codex fixture.
#![cfg(target_os = "linux")]
use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use std::{
    fs,
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt},
        net::UnixListener,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    thread,
    time::{Duration, Instant},
};
use wyss_console::{
    authority::AuthorityService,
    contracts::{WireUuid, control::*, identities::ManagedSessionId},
    control::client::ControlClient,
    lifecycle::{
        ActiveState, AuthorityBootstrap, BuildKind, Digest, Failure, InstallationState, Layout,
        LifecycleView, Limits, ManagerError, Origin, Package, Phase, Provision, ServiceManager,
        ServiceState, Systemd, UNIT_NAME, UnitResult, UnitStatus,
    },
    sessions::{ActivationStage, AgentMode, SessionService},
    turns::TurnService,
};

/// One private service manager and one provider at a time.
static SERIAL: Mutex<()> = Mutex::new(());
fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}
fn id() -> WireUuid {
    WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap()
}
const SYSTEMD: &str = "/usr/lib/systemd/systemd";
const SYSTEMCTL: &str = "/usr/bin/systemctl";

/// A clean account: an empty home, a private runtime directory, and beside them the
/// package a desktop was shipped in.
struct Account {
    root: PathBuf,
    scenario: &'static str,
}
impl Account {
    fn new(scenario: &'static str) -> Self {
        // Short: the manager's socket and the control socket live below it.
        let name = uuid::Uuid::new_v4().simple().to_string();
        let root = std::env::temp_dir().join(format!("wyss-lp-{}", &name[..10]));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        for dir in [
            "run",
            "home",
            "bin",
            "codex-home",
            "package/bin",
            "package/libexec/wyss-console",
        ] {
            fs::DirBuilder::new()
                .mode(0o700)
                .recursive(true)
                .create(root.join(dir))
                .unwrap();
        }
        // The provider the authority will find in the service manager's PATH.
        fs::write(
            root.join("codex-home/models_cache.json"),
            r#"{"fetched_at":"2026-10-07T00:00:00Z","models":[{"slug":"fixture-model","tool_mode":"code_mode_only","shell_type":"unified_exec","apply_patch_tool_type":"freeform","multi_agent_version":"v2","experimental_supported_tools":["send_user_message_async","clock"]}]}"#,
        )
        .unwrap();
        let fake = root.join("bin/codex");
        fs::write(&fake, include_str!("fixtures/codex_runtime.py")).unwrap();
        fs::set_permissions(fake, fs::Permissions::from_mode(0o700)).unwrap();
        let home = root.join("codex-home");
        fs::write(home.join("auth.json"), "FIXTURE-CREDENTIAL").unwrap();
        fs::set_permissions(home.join("auth.json"), fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(home.join("config.toml"), "approval_policy = \"never\"\n").unwrap();
        for dir in ["repo/.git", "repo/.codex", "repo/sub/ws"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        Self { root, scenario }
    }
    fn run(&self) -> PathBuf {
        self.root.join("run")
    }
    fn home(&self) -> PathBuf {
        self.root.join("home")
    }
    fn workspace(&self) -> String {
        self.root.join("repo/sub/ws").to_str().unwrap().to_owned()
    }
    /// The layout a desktop of this account works in: the XDG defaults under its home.
    fn layout(&self) -> Layout {
        Layout::new(
            self.home().join(".local/share"),
            self.home().join(".config"),
            self.run(),
        )
        .unwrap()
    }
    /// Where the authority of this account keeps its store. The session names it
    /// (`XDG_STATE_HOME`), beside the home, which is where the provider fixture looks
    /// for what its namespace hides.
    fn state(&self) -> PathBuf {
        self.root.join("state/wyss-console")
    }
    /// The package's authority: the real `wyss-control`, with `tail` appended so that
    /// two builds of it are two different files that both run.
    fn package(&self, tail: &[u8]) -> Provision {
        let mut bytes = fs::read(env!("CARGO_BIN_EXE_wyss-control")).unwrap();
        bytes.extend_from_slice(tail);
        self.package_of(&bytes)
    }
    fn package_of(&self, bytes: &[u8]) -> Provision {
        let payload = self.root.join("package/libexec/wyss-console/wyss-control");
        let _ = fs::remove_file(&payload);
        fs::write(&payload, bytes).unwrap();
        fs::set_permissions(&payload, fs::Permissions::from_mode(0o755)).unwrap();
        Provision::Packaged(Package::at(payload, Digest::of(bytes)))
    }
    /// The user's service manager: a `systemd --user` of this account alone. Its
    /// environment is what a login session gives one, and what the unit inherits.
    fn manager(&self) -> Manager {
        // A target with nothing in it: no unit of the system's is pulled into the test.
        let units = self.run().join("systemd/user");
        fs::DirBuilder::new()
            .mode(0o700)
            .recursive(true)
            .create(&units)
            .unwrap();
        fs::write(
            units.join("wyss-test.target"),
            "[Unit]\nDescription=Empty target of the lifecycle tests\n",
        )
        .unwrap();
        let log = fs::File::create(self.root.join("manager.log")).unwrap();
        let child = Command::new(SYSTEMD)
            .args(["--user", "--unit=wyss-test.target"])
            .env_clear()
            .env("HOME", self.home())
            .env("XDG_RUNTIME_DIR", self.run())
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.root.join("bin").display()),
            )
            .env("LANG", "C.UTF-8")
            .env("CODEX_HOME", self.root.join("codex-home"))
            .env("WYSS_RUNTIME_FIXTURE", &self.root)
            .env("WYSS_RUNTIME_SCENARIO", self.scenario)
            .env("WYSS_FIXTURE_USER_HOME", self.root.join("codex-home"))
            .env(
                "WYSS_FIXTURE_LAYERS",
                serde_json::to_string(&[self.root.join("repo/.codex")]).unwrap(),
            )
            // What a hostile session might carry. The unit removes it.
            .env("LD_PRELOAD", self.root.join("no-such-library.so"))
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap();
        let manager = Manager {
            child,
            run: self.run(),
            root: self.root.clone(),
        };
        let deadline = Instant::now() + Duration::from_secs(15);
        while Systemd::new(&self.run()).status().is_err() {
            assert!(
                Instant::now() < deadline,
                "the private manager did not start"
            );
            thread::sleep(Duration::from_millis(20));
        }
        // A service manager gives its units a PATH of its own. A desktop session imports
        // the session's into it at login; this is that import, for this account.
        let path = format!("PATH={}:/usr/bin:/bin", self.root.join("bin").display());
        assert!(manager.systemctl(&["set-environment", &path]).0);
        manager
    }
    /// An independent client of whatever authority answers in this account.
    fn client(&self) -> ControlClient {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            match ControlClient::connect_in_runtime(&self.run()) {
                Ok(c) => return c,
                Err(e) => {
                    assert!(Instant::now() < end, "no authority answers: {e}");
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }
    fn answers(&self) -> bool {
        ControlClient::connect_in_runtime(&self.run()).is_ok()
    }
    /// Every entry below `dir` of the account, with its mode.
    fn tree(&self, dir: &Path) -> Vec<(String, u32)> {
        fn walk(root: &Path, at: &Path, out: &mut Vec<(String, u32)>) {
            let Ok(entries) = fs::read_dir(at) else {
                return;
            };
            let mut entries: Vec<_> = entries.flatten().collect();
            entries.sort_by_key(|e| e.file_name());
            for entry in entries {
                let meta = fs::symlink_metadata(entry.path()).unwrap();
                out.push((
                    entry
                        .path()
                        .strip_prefix(root)
                        .unwrap()
                        .display()
                        .to_string(),
                    meta.mode() & 0o7777,
                ));
                if meta.is_dir() {
                    walk(root, &entry.path(), out);
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, dir, &mut out);
        out
    }
}
impl Drop for Account {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// The private `systemd --user`. Dropping it stops the unit, then the manager, and
/// leaves no process of the test behind.
struct Manager {
    child: Child,
    run: PathBuf,
    root: PathBuf,
}
impl Manager {
    fn systemctl(&self, arguments: &[&str]) -> (bool, String) {
        let output = Command::new(SYSTEMCTL)
            .arg("--user")
            .args(arguments)
            .env_clear()
            .env("XDG_RUNTIME_DIR", &self.run)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        (
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
        )
    }
    fn status(&self) -> UnitStatus {
        Systemd::new(&self.run).status().unwrap()
    }
    /// A property of the unit the product's own reading does not ask for.
    fn property(&self, name: &str) -> String {
        let (_, output) = self.systemctl(&["show", UNIT_NAME, &format!("--property={name}")]);
        output
            .trim()
            .strip_prefix(&format!("{name}="))
            .unwrap_or_default()
            .to_owned()
    }
    /// The process the manager runs as the unit, once it runs one.
    fn main_pid(&self) -> u32 {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let status = self.status();
            if let (ActiveState::Active, Some(pid)) = (status.active, status.main_pid) {
                return pid;
            }
            assert!(
                Instant::now() < deadline,
                "the unit is not running: {status:?}"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
    fn wait(&self, done: impl Fn(&UnitStatus) -> bool) -> UnitStatus {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let status = self.status();
            if done(&status) {
                return status;
            }
            assert!(Instant::now() < deadline, "the unit stays {status:?}");
            thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for Manager {
    fn drop(&mut self) {
        let pid = Systemd::new(&self.run)
            .status()
            .ok()
            .and_then(|s| s.main_pid);
        let _ = self.systemctl(&["stop", UNIT_NAME]);
        let _ = self.systemctl(&["exit"]);
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Nothing of this account outlives the test, whatever went wrong in it.
        if let Some(pid) = pid
            && fs::read_link(format!("/proc/{pid}/exe"))
                .is_ok_and(|exe| exe.starts_with(&self.root))
        {
            let _ = kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
        }
    }
}

/// The real manager, with what was asked of it written down.
struct Recorded {
    inner: Systemd,
    calls: Mutex<Vec<&'static str>>,
}
impl Recorded {
    fn asked(&self) -> Vec<&'static str> {
        self.calls.lock().unwrap().clone()
    }
}
impl ServiceManager for Recorded {
    fn status(&self) -> Result<UnitStatus, ManagerError> {
        self.inner.status()
    }
    fn reload(&self) -> Result<(), ManagerError> {
        self.calls.lock().unwrap().push("reload");
        self.inner.reload()
    }
    fn start(&self) -> Result<(), ManagerError> {
        self.calls.lock().unwrap().push("start");
        self.inner.start()
    }
    fn reset_failed(&self) -> Result<(), ManagerError> {
        self.calls.lock().unwrap().push("reset-failed");
        self.inner.reset_failed()
    }
}

/// A desktop process, as far as the authority is concerned: the observer of the
/// connection, and the bootstrap that follows it.
struct Desktop {
    authority: AuthorityService,
    bootstrap: AuthorityBootstrap,
    manager: Arc<Recorded>,
    phases: Mutex<(Option<u64>, Vec<Phase>)>,
}
impl Desktop {
    fn launch(account: &Account, provision: Provision) -> Self {
        let authority = AuthorityService::start(Some(account.run()));
        let manager = Arc::new(Recorded {
            inner: Systemd::new(&account.run()),
            calls: Mutex::new(Vec::new()),
        });
        let bootstrap = AuthorityBootstrap::start_with(
            Arc::new(authority.clone()),
            provision,
            Ok(account.layout()),
            manager.clone(),
            Limits::default(),
        );
        Self {
            authority,
            bootstrap,
            manager,
            phases: Mutex::new((None, Vec::new())),
        }
    }
    /// Follows the lifecycle until `done`, remembering every phase it was shown.
    fn until(&self, what: &str, done: impl Fn(&LifecycleView) -> bool) -> LifecycleView {
        let deadline = Instant::now() + Duration::from_secs(40);
        let mut seen = self.phases.lock().unwrap();
        loop {
            let view = self
                .bootstrap
                .wait_changed(seen.0, Duration::from_millis(200));
            seen.0 = Some(view.revision);
            if seen.1.last() != Some(&view.phase) {
                seen.1.push(view.phase);
            }
            if done(&view) {
                return view;
            }
            assert!(
                Instant::now() < deadline,
                "waiting for {what}: {view:?} after {:?}; link {:?}; unit {:?}",
                seen.1,
                self.authority.view(),
                self.manager.inner.status()
            );
        }
    }
    fn ready(&self) -> LifecycleView {
        self.until("ready", |v| v.phase == Phase::Ready)
    }
    fn failed(&self) -> LifecycleView {
        self.until("a failure", |v| v.phase == Phase::Failed)
    }
    /// The phases shown since the last time this was asked.
    fn phases(&self) -> Vec<Phase> {
        std::mem::take(&mut self.phases.lock().unwrap().1)
    }
    /// The window is closed and the process ends.
    fn close(self) {
        self.authority.shutdown();
        self.bootstrap.shutdown();
    }
}
fn inode(path: &Path) -> u64 {
    fs::symlink_metadata(path).unwrap().ino()
}
fn alive(pid: u32) -> bool {
    kill(Pid::from_raw(pid as i32), None).is_ok()
}
/// A session created and activated in `mode`, the way the window does it.
fn activated(
    sessions: &SessionService,
    turns: &TurnService,
    workspace: &str,
    mode: AgentMode,
) -> ManagedSessionId {
    let session = sessions.prepare(id()).unwrap().id;
    let operation = id();
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        let progress = turns
            .activate(
                operation.clone(),
                session.clone(),
                1,
                workspace.to_owned(),
                mode,
            )
            .unwrap();
        if progress.stage == ActivationStage::Ready {
            return session;
        }
        assert!(
            Instant::now() < deadline,
            "activation at {:?}",
            progress.stage
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_clean_account_gets_an_authority_that_outlives_the_desktop_and_survives_a_crash() {
    let _serial = serial();
    let account = Account::new("approval_command");
    let manager = account.manager();
    let layout = account.layout();

    // Nothing is installed, nothing runs, nothing was ever stored.
    assert!(account.tree(&account.home()).is_empty());
    assert!(!account.run().join("wyss-console").exists());
    assert_eq!(
        manager.status().load,
        wyss_console::lifecycle::LoadState::NotFound
    );

    // The desktop is launched. That is all anyone does.
    let desktop = Desktop::launch(&account, account.package(b""));
    let view = desktop.ready();
    // Whatever of it a reader was shown, it was shown in this order.
    let order = [
        Phase::Checking,
        Phase::Installing,
        Phase::Starting,
        Phase::Connecting,
        Phase::Ready,
    ];
    let phases = desktop.phases();
    let mut rest = order.iter();
    assert!(phases.iter().all(|p| rest.any(|o| o == p)), "{phases:?}");
    assert_eq!(
        (view.origin, view.update_pending, view.failure, view.retry),
        (Some(Origin::Managed), false, None, false)
    );
    assert_eq!(view.diagnostics.build, BuildKind::Packaged);
    assert_eq!(view.diagnostics.installation, InstallationState::Current);
    assert_eq!(view.diagnostics.service, ServiceState::Running);
    assert_eq!(desktop.manager.asked(), ["reload", "start"]);

    // Exactly what was installed, where, and private to the user.
    assert_eq!(
        account.tree(&account.home().join(".local/share")),
        [
            ("wyss-console".to_owned(), 0o700),
            ("wyss-console/authority".to_owned(), 0o700),
            ("wyss-console/authority/.install.lock".to_owned(), 0o600),
            ("wyss-console/authority/wyss-control".to_owned(), 0o700),
        ]
    );
    assert_eq!(
        account.tree(&account.home().join(".config")),
        [
            ("systemd".to_owned(), 0o700),
            ("systemd/user".to_owned(), 0o700),
            ("systemd/user/wyss-control.service".to_owned(), 0o600),
        ]
    );
    assert_eq!(
        fs::read(layout.binary()).unwrap(),
        fs::read(env!("CARGO_BIN_EXE_wyss-control")).unwrap()
    );
    // The manager runs the unit from that file, and the process is that binary: not one
    // found in PATH, not the one of the package, not the one of the build tree.
    let status = manager.status();
    let pid = manager.main_pid();
    assert_eq!(status.fragment, Some(layout.unit()));
    assert_eq!(
        fs::read_link(format!("/proc/{pid}/exe")).unwrap(),
        layout.binary()
    );
    // What the session carried for the loader did not reach the authority.
    let environment = fs::read(format!("/proc/{pid}/environ")).unwrap();
    let environment = String::from_utf8_lossy(&environment);
    assert!(
        !environment.contains("LD_PRELOAD"),
        "the loader was told what to load"
    );
    assert!(environment.contains("XDG_RUNTIME_DIR="));
    // Nothing can enable it: it is started when a desktop needs it.
    assert_eq!(manager.property("UnitFileState"), "static");
    // The authority made its own store and socket, where it always has.
    let database = account.state().join("control.sqlite3");
    assert_eq!(
        fs::symlink_metadata(&database).unwrap().mode() & 0o777,
        0o600
    );
    let socket = fs::symlink_metadata(layout.socket()).unwrap();
    assert!(socket.file_type().is_socket());
    assert_eq!(socket.mode() & 0o777, 0o600);

    // It works: the runtime starts under the unit's hardening, in both modes, the
    // isolated one included.
    let sessions = SessionService::new(Some(account.run()));
    let turns = TurnService::new(sessions.clone());
    let tools = activated(
        &sessions,
        &turns,
        &account.workspace(),
        AgentMode::LocalTools,
    );
    let mut client = account.client();
    assert_eq!(
        client.runtime_status().unwrap().runtime_safety_profile,
        Some(RuntimeSafetyProfile::CodexLocalToolsV1)
    );
    let first = client.hello_status().clone();
    let events = first.last_sequence.map(|s| s.get()).unwrap();
    drop(client);
    // Every process the authority runs for it started in the authority's own empty
    // directory: not in the tree the binary was built in, and not in the user's home.
    let children: Vec<PathBuf> = fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|p| {
            fs::read_to_string(format!("/proc/{p}/stat")).is_ok_and(|stat| {
                // The parent is the second field after the parenthesised name.
                stat.rsplit_once(") ")
                    .and_then(|(_, rest)| rest.split(' ').nth(1)?.parse::<u32>().ok())
                    == Some(pid)
            })
        })
        .filter_map(|p| fs::read_link(format!("/proc/{p}/cwd")).ok())
        .collect();
    assert!(
        !children.is_empty(),
        "the runtime runs as a child of the authority"
    );
    for cwd in &children {
        assert_eq!(cwd, &account.run().join("wyss-console/runtime-cwd"));
    }

    // The window is closed. The authority is not the desktop's child and is not stopped.
    desktop.close();
    thread::sleep(Duration::from_millis(300));
    assert!(alive(pid));
    assert_eq!(manager.main_pid(), pid);
    let mut client = account.client();
    assert_eq!(
        client.hello_status().authority_instance_id,
        first.authority_instance_id
    );
    assert_eq!(
        client.runtime_status().unwrap().actual_state,
        ActualState::Ready
    );
    drop(client);

    // The desktop is opened again: the same authority is used, and nothing is installed,
    // reloaded or started for it.
    let installed = (inode(&layout.binary()), inode(&layout.unit()));
    let desktop = Desktop::launch(&account, account.package(b""));
    let view = desktop.ready();
    assert_eq!(view.origin, Some(Origin::Managed));
    assert_eq!(desktop.manager.asked(), Vec::<&str>::new());
    assert_eq!((inode(&layout.binary()), inode(&layout.unit())), installed);
    assert_eq!(manager.main_pid(), pid);
    assert_eq!(manager.property("NRestarts"), "0");
    desktop.phases();

    // The authority is killed. The service manager restarts it; the desktop starts
    // nothing, and is ready again when the link is.
    kill(Pid::from_raw(pid as i32), Signal::SIGKILL).unwrap();
    desktop.until("the loss", |v| v.phase != Phase::Ready);
    let view = desktop.ready();
    let phases = desktop.phases();
    assert!(
        !phases.contains(&Phase::Installing) && !phases.contains(&Phase::Starting),
        "{phases:?}"
    );
    assert_eq!(view.origin, Some(Origin::Managed));
    assert_eq!(desktop.manager.asked(), Vec::<&str>::new());
    let restarted = manager.main_pid();
    assert_ne!(restarted, pid);
    assert_eq!(manager.property("NRestarts"), "1");
    // Another process, the same canonical store: the same stream, continued, and the
    // session that was created before the crash.
    let client = account.client();
    let second = client.hello_status().clone();
    assert_ne!(second.authority_instance_id, first.authority_instance_id);
    assert_eq!(second.stream_id, first.stream_id);
    assert!(second.last_sequence.map(|s| s.get()).unwrap() > events);
    drop(client);
    let page = sessions.page(None).unwrap();
    assert!(page.sessions.iter().any(|s| s.id == tools));
    assert_eq!((inode(&layout.binary()), inode(&layout.unit())), installed);

    // Stopped cleanly, by whoever may: that is a decision and it is respected. It is not
    // restarted by the manager, and not by the desktop.
    assert!(manager.systemctl(&["stop", UNIT_NAME]).0);
    let view = desktop.failed();
    assert_eq!((view.failure, view.retry), (Some(Failure::Stopped), true));
    thread::sleep(Duration::from_millis(1500));
    assert_eq!(manager.status().active, ActiveState::Inactive);
    assert!(!account.answers());
    assert_eq!(desktop.manager.asked(), Vec::<&str>::new());
    // Retry is the user asking.
    desktop.bootstrap.retry();
    desktop.until("the retry", |v| v.phase != Phase::Failed);
    assert_eq!(desktop.ready().origin, Some(Origin::Managed));
    assert_eq!(desktop.manager.asked(), ["reload", "start"]);
    let client = account.client();
    assert_eq!(client.hello_status().stream_id, first.stream_id);
    drop(client);
    assert!(
        sessions
            .page(None)
            .unwrap()
            .sessions
            .iter()
            .any(|s| s.id == tools)
    );
    desktop.close();
    sessions.shutdown();
    drop(manager);
    assert!(!account.answers());
}

#[test]
fn desktops_that_start_at_once_get_one_authority() {
    let _serial = serial();
    let account = Account::new("happy");
    let manager = account.manager();
    let layout = account.layout();
    let package = account.package(b"");
    let desktops: Vec<Desktop> = thread::scope(|scope| {
        let launches: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| Desktop::launch(&account, package.clone())))
            .collect();
        launches.into_iter().map(|l| l.join().unwrap()).collect()
    });
    for desktop in &desktops {
        assert_eq!(desktop.ready().origin, Some(Origin::Managed));
    }
    // One process owns the store and the socket, and every desktop is talking to it.
    let pid = manager.main_pid();
    let instance = account
        .client()
        .hello_status()
        .authority_instance_id
        .clone();
    for _ in 0..4 {
        assert_eq!(
            account.client().hello_status().authority_instance_id,
            instance
        );
    }
    let running: Vec<u32> = fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|p| fs::read_link(format!("/proc/{p}/exe")).is_ok_and(|exe| exe == layout.binary()))
        .collect();
    assert_eq!(running, [pid]);
    assert_eq!(manager.property("NRestarts"), "0");
    // One whole installation: no temporary file, no second copy.
    assert_eq!(account.tree(&account.home().join(".local/share")).len(), 4);
    assert_eq!(account.tree(&account.home().join(".config")).len(), 3);
    assert_eq!(
        fs::read(layout.binary()).unwrap(),
        fs::read(env!("CARGO_BIN_EXE_wyss-control")).unwrap()
    );
    // Starting was asked at most once by each, and the manager ran it once.
    for desktop in &desktops {
        assert!(
            desktop
                .manager
                .asked()
                .iter()
                .filter(|c| **c == "start")
                .count()
                <= 1
        );
    }
    for desktop in desktops {
        desktop.close();
    }
    assert!(alive(pid));
}

#[test]
fn an_authority_that_keeps_failing_is_restarted_a_bounded_number_of_times_and_no_more() {
    let _serial = serial();
    let account = Account::new("happy");
    let manager = account.manager();
    let runs = || {
        fs::read_to_string(account.home().join("runs"))
            .unwrap_or_default()
            .lines()
            .count()
    };
    // An authority that ends at once, saying it failed.
    let crashing = b"#!/bin/sh\necho run >> \"$HOME/runs\"\nexit 1\n";
    let desktop = Desktop::launch(&account, account.package_of(crashing));
    let view = desktop.failed();
    assert_eq!(
        (view.failure, view.retry),
        (Some(Failure::ExitedRepeatedly), true)
    );
    assert_eq!(view.diagnostics.service, ServiceState::Failed);
    let status = manager.status();
    assert_eq!(
        (status.active, status.result),
        (ActiveState::Failed, UnitResult::StartLimitHit)
    );
    // The manager's limit, and nobody on top of it: the desktop asked once.
    assert_eq!(runs(), 5);
    assert_eq!(desktop.manager.asked(), ["reload", "start"]);
    thread::sleep(Duration::from_secs(3));
    assert_eq!(runs(), 5);
    assert_eq!(desktop.manager.asked(), ["reload", "start"]);
    assert_eq!(desktop.bootstrap.view().phase, Phase::Failed);

    // Another desktop opens: the manager gave up on this unit, and it is not started
    // again by itself.
    let second = Desktop::launch(&account, account.package_of(crashing));
    assert_eq!(second.failed().failure, Some(Failure::ExitedRepeatedly));
    assert_eq!(second.manager.asked(), Vec::<&str>::new());
    assert_eq!(runs(), 5);
    second.close();

    // Retry is the user asking: one more round of the same limit, and it ends again.
    desktop.bootstrap.retry();
    desktop.until("the retry", |v| v.phase != Phase::Failed);
    assert_eq!(desktop.failed().failure, Some(Failure::ExitedRepeatedly));
    assert_eq!(runs(), 10);
    assert_eq!(
        desktop.manager.asked(),
        ["reload", "start", "reload", "reset-failed", "start"]
    );
    desktop.close();
    assert!(manager.systemctl(&["reset-failed", UNIT_NAME]).0);

    // An authority that says another start cannot succeed is not started twice.
    fs::remove_file(account.home().join("runs")).unwrap();
    let refusing = b"#!/bin/sh\necho run >> \"$HOME/runs\"\nexit 2\n";
    let desktop = Desktop::launch(&account, account.package_of(refusing));
    assert_eq!(desktop.failed().failure, Some(Failure::StartupFailed));
    thread::sleep(Duration::from_millis(2500));
    assert_eq!(runs(), 1);
    assert_eq!(manager.property("NRestarts"), "0");
    desktop.close();
}

#[test]
fn without_a_service_manager_it_fails_safely_and_writes_nothing() {
    let _serial = serial();
    let account = Account::new("happy");
    // No manager was started for this account.
    let began = Instant::now();
    let desktop = Desktop::launch(&account, account.package(b""));
    let view = desktop.failed();
    assert!(
        began.elapsed() < Duration::from_secs(15),
        "{:?}",
        began.elapsed()
    );
    assert_eq!(
        (view.failure, view.retry),
        (Some(Failure::ManagerUnavailable), true)
    );
    assert_eq!(view.diagnostics.service, ServiceState::Unavailable);
    assert!(account.tree(&account.home()).is_empty());
    assert!(!account.answers());
    desktop.bootstrap.retry();
    desktop.until("the retry", |v| v.phase != Phase::Failed);
    assert_eq!(desktop.failed().failure, Some(Failure::ManagerUnavailable));
    assert!(account.tree(&account.home()).is_empty());
    // No child of the desktop was left running as an authority instead.
    assert!(!account.run().join("wyss-console/control.sock").exists());
    desktop.close();
}

/// An authority somebody started by hand, the way a developer does.
struct Foreign(Child);
impl Foreign {
    fn start(account: &Account) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_wyss-control"))
            .env_clear()
            .env("HOME", account.home())
            .env("XDG_RUNTIME_DIR", account.run())
            .env("XDG_STATE_HOME", account.root.join("state"))
            .env("PATH", account.root.join("bin"))
            .env("CODEX_HOME", account.root.join("codex-home"))
            .env("WYSS_RUNTIME_FIXTURE", &account.root)
            .env("WYSS_RUNTIME_SCENARIO", account.scenario)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        drop(account.client());
        Self(child)
    }
}
impl Drop for Foreign {
    fn drop(&mut self) {
        let _ = kill(Pid::from_raw(self.0.id() as i32), Signal::SIGTERM);
        let deadline = Instant::now() + Duration::from_secs(8);
        while self.0.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn an_authority_somebody_else_started_is_used_and_never_replaced_or_stopped() {
    let _serial = serial();
    let account = Account::new("happy");
    let manager = account.manager();
    let foreign = Foreign::start(&account);
    let instance = account
        .client()
        .hello_status()
        .authority_instance_id
        .clone();

    let desktop = Desktop::launch(&account, account.package(b"newer"));
    let view = desktop.ready();
    assert_eq!(
        (view.origin, view.update_pending),
        (Some(Origin::External), false)
    );
    assert_eq!(view.diagnostics.installation, InstallationState::Absent);
    assert_eq!(view.diagnostics.service, ServiceState::NotInstalled);
    // Nothing was installed beside it and nothing was asked of the manager.
    assert!(
        account
            .tree(&account.home().join(".local/share"))
            .is_empty()
    );
    assert!(!account.home().join(".config").exists());
    assert_eq!(desktop.manager.asked(), Vec::<&str>::new());
    assert_eq!(
        manager.status().load,
        wyss_console::lifecycle::LoadState::NotFound
    );
    desktop.close();
    assert_eq!(
        account.client().hello_status().authority_instance_id,
        instance
    );

    // A desktop that finds it gone starts its own. One that then meets a second
    // authority started by hand finds the store and the socket already owned: the hand
    // started one is refused by the authority's own locks, not by the desktop.
    drop(foreign);
    assert!(!account.answers());
    let desktop = Desktop::launch(&account, account.package(b""));
    assert_eq!(desktop.ready().origin, Some(Origin::Managed));
    let managed = manager.main_pid();
    let second = Command::new(env!("CARGO_BIN_EXE_wyss-control"))
        .env_clear()
        .env("HOME", account.home())
        .env("XDG_RUNTIME_DIR", account.run())
        .env("XDG_STATE_HOME", account.root.join("state"))
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(second.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&second.stderr).trim(),
        "wyss-control: already_running"
    );
    assert_eq!(manager.main_pid(), managed);
    desktop.close();
}

/// Something that answers the control socket and is not an authority this build can
/// work with: it speaks the framing, under another protocol version.
fn impostor(account: &Account) -> (thread::JoinHandle<()>, Arc<Mutex<usize>>) {
    let dir = account.run().join("wyss-console");
    fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
    let listener = UnixListener::bind(dir.join("control.sock")).unwrap();
    fs::set_permissions(dir.join("control.sock"), fs::Permissions::from_mode(0o600)).unwrap();
    let answered = Arc::new(Mutex::new(0usize));
    let count = answered.clone();
    let handle = thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut header = [0u8; 4];
            if stream.read_exact(&mut header).is_err() {
                // The stop signal of the test, or a probe that sent nothing.
                if *count.lock().unwrap() == usize::MAX {
                    return;
                }
                continue;
            }
            let mut body = vec![0u8; u32::from_be_bytes(header) as usize];
            if stream.read_exact(&mut body).is_err() {
                continue;
            }
            let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let answer = serde_json::json!({
                "schema_version": 2,
                "request_id": request["request_id"],
                "authority_instance_id": "99999999-9999-4999-8999-999999999999",
                "result": {"kind": "hello", "value": {}},
            })
            .to_string();
            let _ = stream.write_all(&(answer.len() as u32).to_be_bytes());
            let _ = stream.write_all(answer.as_bytes());
            let mut count = count.lock().unwrap();
            if *count != usize::MAX {
                *count += 1;
            }
        }
    });
    (handle, answered)
}

#[test]
fn an_incompatible_authority_is_said_and_nothing_is_done_about_it() {
    let _serial = serial();
    let account = Account::new("happy");
    let manager = account.manager();
    let (handle, answered) = impostor(&account);
    let socket = inode(&account.layout().socket());

    let desktop = Desktop::launch(&account, account.package(b""));
    let view = desktop.until("incompatible", |v| v.phase == Phase::Incompatible);
    assert_eq!(
        (view.failure, view.retry, view.update_pending),
        (None, false, false)
    );
    // It is not this installation's: the manager runs nothing.
    assert_eq!(view.origin, Some(Origin::External));
    thread::sleep(Duration::from_millis(600));
    desktop.bootstrap.retry();
    thread::sleep(Duration::from_millis(300));
    assert_eq!(desktop.bootstrap.view().phase, Phase::Incompatible);
    // Nothing was installed over it, nothing was started against it, and its socket is
    // the one it made.
    assert_eq!(desktop.manager.asked(), Vec::<&str>::new());
    assert!(account.tree(&account.home()).is_empty());
    assert_eq!(
        manager.status().load,
        wyss_console::lifecycle::LoadState::NotFound
    );
    assert_eq!(inode(&account.layout().socket()), socket);
    assert!(*answered.lock().unwrap() >= 1);
    desktop.close();
    *answered.lock().unwrap() = usize::MAX;
    let _ = std::os::unix::net::UnixStream::connect(account.layout().socket());
    handle.join().unwrap();
}

#[test]
fn a_newer_authority_is_installed_on_a_cold_start_never_under_work_and_the_store_survives() {
    let _serial = serial();
    // This provider keeps a turn running until it is cancelled.
    let account = Account::new("turn_long");
    let manager = account.manager();
    let layout = account.layout();
    let one = fs::read(env!("CARGO_BIN_EXE_wyss-control")).unwrap();
    let two = [one.as_slice(), b"build two"].concat();

    // Build one is installed and works.
    let desktop = Desktop::launch(&account, account.package_of(&one));
    assert_eq!(desktop.ready().origin, Some(Origin::Managed));
    let sessions = SessionService::new(Some(account.run()));
    let turns = TurnService::new(sessions.clone());
    let session = activated(
        &sessions,
        &turns,
        &account.workspace(),
        AgentMode::Inference,
    );
    let turn = turns
        .start(id(), session.clone(), 1, "keep going".into())
        .unwrap()
        .id;
    let deadline = Instant::now() + Duration::from_secs(10);
    while turns.read(turn.clone(), None, false).unwrap().turn.state != TurnState::Running {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    }
    let pid = manager.main_pid();
    let stream = account.client().hello_status().stream_id.clone();
    let database = inode(&account.state().join("control.sqlite3"));
    let installed = inode(&layout.binary());
    desktop.close();

    // The desktop is updated and opened while a turn runs under the old authority. It
    // connects to it, says a newer one is waiting, and changes nothing.
    let desktop = Desktop::launch(&account, account.package_of(&two));
    let view = desktop.ready();
    assert_eq!(view.origin, Some(Origin::Managed));
    assert_eq!(view.diagnostics.installation, InstallationState::Outdated);
    assert!(view.update_pending);
    thread::sleep(Duration::from_millis(500));
    assert_eq!(desktop.manager.asked(), Vec::<&str>::new());
    assert_eq!(manager.main_pid(), pid);
    assert_eq!(manager.property("NRestarts"), "0");
    assert_eq!(inode(&layout.binary()), installed);
    assert_eq!(fs::read(layout.binary()).unwrap(), one);
    assert_eq!(
        turns.read(turn.clone(), None, false).unwrap().turn.state,
        TurnState::Running
    );
    // It stays that way when the work is over too: a running authority is not replaced.
    let revision = turns.read(turn.clone(), None, false).unwrap().turn.revision;
    turns.cancel(id(), turn.clone(), revision).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while turns
        .read(turn.clone(), None, false)
        .unwrap()
        .turn
        .cancellable
    {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    }
    desktop.bootstrap.retry();
    thread::sleep(Duration::from_millis(500));
    assert_eq!(desktop.manager.asked(), Vec::<&str>::new());
    assert_eq!(
        (manager.main_pid(), inode(&layout.binary())),
        (pid, installed)
    );
    assert!(desktop.bootstrap.view().update_pending);
    desktop.close();

    // The session ends, or the user logs out: the authority is stopped. The next desktop
    // finds none, installs the one it carries, and starts that.
    assert!(manager.systemctl(&["stop", UNIT_NAME]).0);
    manager.wait(|s| s.active == ActiveState::Inactive);
    let desktop = Desktop::launch(&account, account.package_of(&two));
    let view = desktop.ready();
    assert_eq!(desktop.manager.asked(), ["reload", "start"]);
    assert_eq!(
        (
            view.origin,
            view.update_pending,
            view.diagnostics.installation
        ),
        (Some(Origin::Managed), false, InstallationState::Current)
    );
    assert_eq!(fs::read(layout.binary()).unwrap(), two);
    assert_ne!(inode(&layout.binary()), installed);
    let updated = manager.main_pid();
    assert_ne!(updated, pid);
    assert_eq!(
        fs::read_link(format!("/proc/{updated}/exe")).unwrap(),
        layout.binary()
    );
    // The canonical store is the one it was: the same file, the same stream, the session
    // and its turn. Installing wrote two files and read nothing else.
    assert_eq!(inode(&account.state().join("control.sqlite3")), database);
    assert_eq!(account.client().hello_status().stream_id, stream);
    assert!(
        sessions
            .page(None)
            .unwrap()
            .sessions
            .iter()
            .any(|s| s.id == session)
    );
    assert_eq!(turns.list(session).unwrap().turns[0].id, turn);
    assert_eq!(account.tree(&account.home().join(".local/share")).len(), 4);
    desktop.close();
    sessions.shutdown();
}

#[test]
fn a_dead_socket_and_a_half_done_install_are_recovered_on_the_next_launch() {
    let _serial = serial();
    let account = Account::new("happy");
    let manager = account.manager();
    let layout = account.layout();
    let bytes = fs::read(env!("CARGO_BIN_EXE_wyss-control")).unwrap();

    // A socket nobody listens on, left by an authority that was killed.
    let dir = account.run().join("wyss-console");
    fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
    drop(UnixListener::bind(layout.socket()).unwrap());
    fs::set_permissions(layout.socket(), fs::Permissions::from_mode(0o600)).unwrap();
    let dead = inode(&layout.socket());
    // An install that was interrupted after its temporary file was written.
    wyss_console::lifecycle::install::binary(&layout, b"half").unwrap();
    fs::rename(
        layout.binary(),
        layout.binary_dir().join(".wyss-control.dead.tmp"),
    )
    .unwrap();

    let desktop = Desktop::launch(&account, account.package(b""));
    assert_eq!(desktop.ready().origin, Some(Origin::Managed));
    assert_eq!(fs::read(layout.binary()).unwrap(), bytes);
    assert_eq!(account.tree(&account.home().join(".local/share")).len(), 4);
    // The authority replaced the dead socket itself, by its own rules. The desktop
    // removed nothing.
    assert_ne!(inode(&layout.socket()), dead);
    desktop.close();
    let stop = || {
        assert!(manager.systemctl(&["stop", UNIT_NAME]).0);
        manager.wait(|s| s.active == ActiveState::Inactive);
    };

    // Interrupted after the binary was renamed and before the unit was written.
    stop();
    fs::remove_file(layout.unit()).unwrap();
    assert!(manager.systemctl(&["daemon-reload"]).0);
    let desktop = Desktop::launch(&account, account.package(b""));
    assert_eq!(desktop.ready().origin, Some(Origin::Managed));
    assert_eq!(desktop.manager.asked(), ["reload", "start"]);
    desktop.close();

    // Interrupted after the unit was renamed and before the manager was told: it still
    // has the unit of another build loaded.
    stop();
    fs::write(
        layout.unit(),
        format!(
            "{}\n[Service]\nExecStart=/nonexistent/wyss-control\n",
            wyss_console::lifecycle::install::UNIT_MARKER
        ),
    )
    .unwrap();
    assert!(manager.systemctl(&["daemon-reload"]).0);
    wyss_console::lifecycle::install::unit(&layout).unwrap();
    let desktop = Desktop::launch(&account, account.package(b""));
    assert_eq!(desktop.ready().origin, Some(Origin::Managed));
    assert_eq!(
        fs::read_link(format!("/proc/{}/exe", manager.main_pid())).unwrap(),
        layout.binary()
    );
    desktop.close();

    // Interrupted after the start was asked for: the unit runs, and the next desktop
    // neither installs nor starts.
    let desktop = Desktop::launch(&account, account.package(b""));
    assert_eq!(desktop.ready().origin, Some(Origin::Managed));
    assert_eq!(desktop.manager.asked(), Vec::<&str>::new());
    desktop.close();
}
