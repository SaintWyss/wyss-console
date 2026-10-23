//! M1-02..M1-12 against a real locked store on disk and a scripted owner. Nothing here starts a
//! host, an agent or a provider: the owner is a recorder whose launches are fail-on-call unless a
//! test scripts one, and every store lives in a directory made for the test and removed with it.
use super::{
    apply::{Disclosures, Engine, Env, Purpose, Selections, empty_store},
    command::{Envelope, ErrorCode, Failure, Receipt},
    owner::{
        AuthorityRef, FolderError, HostView, LaunchError, ManagedAdvert, ManagedLaunch,
        ManagedLaunched, ManagedRow, NativeLaunch, NativeLaunched, Owner, OwnerError, RootError,
        RootInfo, ShellCwd, ShellLaunched, TermRead, TerminalRow,
    },
    store::{Faults, Loaded, Point, Store, StoreError},
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU32, Ordering},
    },
};
use wyss_console::contracts::m1::{
    StoreV1,
    records::{CheckoutKind, ManagedProtocol, ManagedSafetyProfile},
    scalar::Counter,
};

// ---- a directory that goes away with the test --------------------------------------------

pub(super) struct TestDir(pub PathBuf);

impl TestDir {
    pub(super) fn new() -> Self {
        let path = std::env::temp_dir().join(format!("wyss-m1-{}", uuid::Uuid::new_v4().simple()));
        crate::native::store::private_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---- deterministic environment -----------------------------------------------------------

static NEXT_BASE: AtomicU32 = AtomicU32::new(0x1000);

pub(super) struct FakeEnv {
    next: AtomicU32,
    live: Mutex<HashSet<String>>,
    claim: String,
}

impl FakeEnv {
    pub(super) fn new() -> Self {
        Self::with_claim("c1:1:1:fake")
    }

    /// Another process of the same store: its claim is its own, and the first one's is not live.
    pub(super) fn with_claim(claim: &str) -> Self {
        Self {
            next: AtomicU32::new(NEXT_BASE.fetch_add(0x10000, Ordering::SeqCst)),
            live: Mutex::new(HashSet::new()),
            claim: claim.to_owned(),
        }
    }

    pub(super) fn mark_live(&self, claim: &str) {
        self.live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(claim.to_owned());
    }
}

impl Env for FakeEnv {
    fn uuid(&self) -> String {
        let n = self.next.fetch_add(1, Ordering::SeqCst);
        format!("{:08x}-0000-4000-8000-{:012x}", 0xfeed, n)
    }

    fn now(&self) -> String {
        "2026-10-23T12:00:00Z".to_owned()
    }

    fn claim(&self) -> String {
        self.claim.clone()
    }

    fn claim_is_live(&self, claim: &str) -> bool {
        claim == self.claim
            || self
                .live
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .contains(claim)
    }
}

// ---- a scripted owner --------------------------------------------------------------------

/// The Managed authority a launch goes to unless a test says otherwise.
pub(super) const STREAM_P1: &str = "bbbbbbbb-0000-4000-8000-000000000001";

pub(super) fn advert(
    protocol: ManagedProtocol,
    stream: &str,
    profiles: &[ManagedSafetyProfile],
) -> ManagedAdvert {
    ManagedAdvert {
        authority: AuthorityRef {
            protocol,
            store_stream_id: stream.to_owned(),
        },
        profiles: profiles.to_vec(),
    }
}

pub(super) struct Script {
    /// Runs once, the next time a folder is looked at: another window acting in between.
    pub on_root: Option<Box<dyn FnOnce() + Send>>,
    /// What the Managed authority a launch would go to shows it serves.
    pub advert: Result<ManagedAdvert, OwnerError>,
    /// Where a running shell is, by terminal ID.
    pub cwd: HashMap<String, Result<ShellCwd, OwnerError>>,
    /// A launch waits here (outside the script's lock) until the test releases it.
    pub gate: Option<std::sync::mpsc::Receiver<()>>,
    pub epoch: String,
    pub host: Option<Result<Vec<TerminalRow>, OwnerError>>,
    pub managed: Result<Vec<ManagedRow>, OwnerError>,
    pub roots: HashMap<String, Result<RootInfo, RootError>>,
    pub native: Vec<Result<NativeLaunched, LaunchError>>,
    pub shell: Vec<Result<ShellLaunched, LaunchError>>,
    pub prepared: Vec<Result<ManagedLaunched, LaunchError>>,
    pub calls: Vec<String>,
    /// The authority each Managed launch named.
    pub authorities: Vec<AuthorityRef>,
    pub folders: Vec<(String, String)>,
    pub written: Vec<(String, Vec<u8>)>,
    pub resized: Vec<(String, u16, u16)>,
}

pub(super) struct FakeOwner(pub Mutex<Script>);

impl FakeOwner {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self(Mutex::new(Script {
            on_root: None,
            advert: Ok(advert(
                ManagedProtocol::P1,
                STREAM_P1,
                &[
                    ManagedSafetyProfile::Inference,
                    ManagedSafetyProfile::LocalTools,
                ],
            )),
            cwd: HashMap::new(),
            gate: None,
            epoch: "h1:1:1:100:1000".to_owned(),
            host: Some(Ok(Vec::new())),
            managed: Ok(Vec::new()),
            roots: HashMap::new(),
            native: Vec::new(),
            shell: Vec::new(),
            prepared: Vec::new(),
            calls: Vec::new(),
            authorities: Vec::new(),
            folders: Vec::new(),
            written: Vec::new(),
            resized: Vec::new(),
        })))
    }

    pub(super) fn script<T>(&self, change: impl FnOnce(&mut Script) -> T) -> T {
        change(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }

    pub(super) fn calls(&self) -> Vec<String> {
        self.script(|s| s.calls.clone())
    }

    pub(super) fn launches(&self) -> usize {
        self.calls()
            .iter()
            .filter(|c| c.starts_with("launch_"))
            .count()
    }

    pub(super) fn add_root(&self, path: &str, device: u64, inode: u64) {
        self.script(|s| {
            s.roots.insert(
                path.to_owned(),
                Ok(RootInfo {
                    path: path.to_owned(),
                    device,
                    inode,
                    kind: CheckoutKind::Git,
                    native_eligible: true,
                    config_eligible: true,
                }),
            );
        });
    }
}

impl Owner for FakeOwner {
    fn host(&self, expected: Option<&str>) -> Result<HostView, OwnerError> {
        self.script(|s| {
            s.calls.push("host".into());
            if expected.is_some_and(|e| e != s.epoch) {
                return Err(OwnerError::HostChanged);
            }
            match s.host.clone().expect("a host script") {
                Ok(terminals) => Ok(HostView {
                    epoch: s.epoch.clone(),
                    features: vec!["profile_effort".into()],
                    terminals,
                }),
                Err(error) => Err(error),
            }
        })
    }

    fn managed(&self) -> Result<Vec<ManagedRow>, OwnerError> {
        self.script(|s| {
            s.calls.push("managed".into());
            s.managed.clone()
        })
    }

    fn managed_advert(&self) -> Result<ManagedAdvert, OwnerError> {
        self.script(|s| {
            s.calls.push("managed_advert".into());
            s.advert.clone()
        })
    }

    fn shell_cwd(&self, epoch: &str, terminal: &str) -> Result<ShellCwd, OwnerError> {
        self.script(|s| {
            s.calls.push(format!("shell_cwd {terminal}"));
            if epoch != s.epoch {
                return Err(OwnerError::HostChanged);
            }
            s.cwd
                .get(terminal)
                .cloned()
                .unwrap_or(Err(OwnerError::Refused))
        })
    }

    fn root(&self, path: &str) -> Result<RootInfo, RootError> {
        if let Some(act) = self.script(|s| s.on_root.take()) {
            act();
        }
        self.script(|s| {
            s.calls.push("root".into());
            s.roots
                .get(path)
                .cloned()
                .unwrap_or(Err(RootError::Missing))
        })
    }

    fn create_folder(&self, parent: &str, name: &str) -> Result<String, FolderError> {
        self.script(|s| {
            s.calls.push("create_folder".into());
            s.folders.push((parent.to_owned(), name.to_owned()));
            Ok(format!("{parent}/{name}"))
        })
    }

    fn launch_native(
        &self,
        launch: &NativeLaunch,
        epoch: &str,
    ) -> Result<NativeLaunched, LaunchError> {
        let gate = self.script(|s| {
            s.calls
                .push(format!("launch_native {} {epoch}", launch.path));
            s.gate.take()
        });
        if let Some(gate) = gate {
            let _ = gate.recv();
        }
        self.script(|s| {
            if s.native.is_empty() {
                panic!("an unscripted Native launch reached the owner");
            }
            s.native.remove(0)
        })
    }

    fn launch_shell(&self, path: &str, epoch: &str) -> Result<ShellLaunched, LaunchError> {
        self.script(|s| {
            s.calls.push(format!("launch_shell {path} {epoch}"));
            if s.shell.is_empty() {
                panic!("an unscripted shell launch reached the owner");
            }
            s.shell.remove(0)
        })
    }

    fn term_read(
        &self,
        epoch: &str,
        terminal: &str,
        from: u64,
        _: u64,
    ) -> Result<TermRead, OwnerError> {
        self.script(|s| {
            s.calls.push(format!("term_read {terminal}"));
            if epoch != s.epoch {
                return Err(OwnerError::HostChanged);
            }
            Ok(TermRead {
                data: b"out".to_vec(),
                next: from + 3,
                eof: false,
                truncated: false,
            })
        })
    }

    fn term_write(&self, epoch: &str, terminal: &str, data: &[u8]) -> Result<(), OwnerError> {
        self.script(|s| {
            s.calls.push(format!("term_write {terminal}"));
            if epoch != s.epoch {
                return Err(OwnerError::HostChanged);
            }
            s.written.push((terminal.to_owned(), data.to_vec()));
            Ok(())
        })
    }

    fn term_resize(
        &self,
        epoch: &str,
        terminal: &str,
        rows: u16,
        cols: u16,
    ) -> Result<(), OwnerError> {
        self.script(|s| {
            s.calls.push(format!("term_resize {terminal}"));
            if epoch != s.epoch {
                return Err(OwnerError::HostChanged);
            }
            s.resized.push((terminal.to_owned(), rows, cols));
            Ok(())
        })
    }

    fn launch_managed(&self, launch: &ManagedLaunch) -> Result<ManagedLaunched, LaunchError> {
        self.script(|s| {
            s.calls
                .push(format!("launch_managed {} {}", launch.path, launch.intent));
            s.authorities.push(launch.authority.clone());
            if s.prepared.is_empty() {
                panic!("an unscripted Managed launch reached the owner");
            }
            s.prepared.remove(0)
        })
    }
}

// ---- the world ---------------------------------------------------------------------------

pub(super) const ROOT_A: &str = "/home/fixture/work/a";
pub(super) const ROOT_B: &str = "/home/fixture/work/b";

pub(super) struct World {
    pub dir: TestDir,
    pub store: Store,
    pub owner: Arc<FakeOwner>,
    pub env: Arc<FakeEnv>,
    pub selections: Selections,
    pub disclosures: Disclosures,
    pub window_revision: u64,
}

impl World {
    pub(super) fn new() -> Self {
        let dir = TestDir::new();
        let store = Store::open(dir.0.join("v1")).unwrap();
        let env = Arc::new(FakeEnv::new());
        store.create(empty_store(&*env).unwrap()).unwrap();
        let owner = FakeOwner::new();
        owner.add_root(ROOT_A, 64768, 1001);
        owner.add_root(ROOT_B, 64768, 1002);
        Self {
            dir,
            store,
            owner,
            env,
            selections: Selections::default(),
            disclosures: Disclosures::default(),
            window_revision: 1,
        }
    }

    pub(super) fn with_faults(mut self, faults: Faults) -> Self {
        self.store = Store::open(self.dir.0.join("v1"))
            .unwrap()
            .with_faults(faults);
        self
    }

    pub(super) fn engine(&self) -> Engine<'_> {
        Engine {
            store: &self.store,
            owner: &*self.owner,
            env: &*self.env,
            selections: &self.selections,
            disclosures: &self.disclosures,
        }
    }

    pub(super) fn now(&self) -> StoreV1 {
        match self.store.read().unwrap() {
            Loaded::Ready(store) => *store,
            other => panic!("the store must be ready: {other:?}"),
        }
    }

    /// A native pick as the dialog would issue it.
    pub(super) fn pick(&self, path: &str, purpose: Purpose) -> String {
        let token = format!("token-{}", self.env.uuid());
        self.selections.issue(
            token.clone(),
            path.to_owned(),
            purpose,
            self.window_revision,
        );
        token
    }

    pub(super) fn context(&self, project: Option<&str>, workspace: Option<&str>) -> Value {
        let store = self.now();
        let binding = project.and_then(|p| {
            store
                .projects
                .iter()
                .find(|x| x.id.as_str() == p)
                .and_then(|x| x.default_binding_id.clone())
                .and_then(|b| store.bindings.iter().find(|x| x.id == b).cloned())
        });
        json!({
            "hostId": store.host_id.as_str(),
            "projectId": project,
            "workspaceId": workspace,
            "bindingId": binding.as_ref().map(|b| b.id.as_str().to_owned()),
            "bindingRevision": binding.as_ref().map(|b| b.revision.get().to_string()),
            "selectionRevision": self.window_revision.to_string(),
        })
    }

    pub(super) fn envelope(
        &self,
        operation: &str,
        revision: Counter,
        context: Value,
        command: Value,
    ) -> Envelope {
        serde_json::from_value(json!({
            "schema": 1,
            "operationId": operation,
            "expectedStoreRevision": revision.get().to_string(),
            "context": context,
            "command": command,
        }))
        .expect("a well-formed envelope")
    }

    /// Sends a command at the store's current revision, with a fresh operation ID.
    pub(super) fn send(&self, context: Value, command: Value) -> Result<Receipt, Failure> {
        let revision = self.now().revision;
        let operation = self.env.uuid();
        self.engine()
            .execute(self.envelope(&operation, revision, context, command))
    }

    /// Registers `path` and returns (project, binding, workspace) IDs.
    pub(super) fn register(&self, path: &str) -> (String, String, String) {
        let token = self.pick(path, Purpose::Register);
        let receipt = self
            .send(
                self.context(None, None),
                json!({"kind": "registerExisting", "selectionToken": token}),
            )
            .expect("registered");
        let role = |name: &str| {
            receipt
                .created
                .iter()
                .find(|c| c.role == name)
                .map(|c| c.id.clone())
                .unwrap_or_else(|| panic!("no {name} created"))
        };
        (role("project"), role("binding"), role("workspace"))
    }
}

#[track_caller]
pub(super) fn refused(result: Result<Receipt, Failure>, code: ErrorCode) -> Failure {
    match result {
        Ok(_) => panic!("expected {code:?}, the command succeeded"),
        Err(failure) => {
            assert_eq!(failure.code, code, "{failure:?}");
            failure
        }
    }
}

pub(super) fn leaf_json(id: &str, association: Value) -> Value {
    json!({"id": id, "title": "Pane", "associationId": association})
}

pub(super) fn native_row(terminal: &str, session: Option<&str>) -> TerminalRow {
    TerminalRow {
        terminal_id: terminal.to_owned(),
        running: true,
        cwd: None,
        native_session: session.map(str::to_owned),
        pid: None,
    }
}

pub(super) fn managed_row(
    protocol: ManagedProtocol,
    stream: &str,
    session: &str,
    effective: Option<ManagedSafetyProfile>,
) -> ManagedRow {
    ManagedRow {
        protocol,
        store_stream_id: stream.to_owned(),
        session_id: session.to_owned(),
        effective_profile: effective,
        attached: effective.is_some(),
    }
}

pub(super) fn unusable(result: Result<StoreV1, StoreError>) -> super::store::Unusable {
    match result {
        Err(StoreError::Unusable(unusable)) => unusable,
        other => panic!("expected an unusable store, got {other:?}"),
    }
}

mod acl_tests;
mod adoption_tests;
mod bench_tests;
mod launch_tests;
mod migration_tests;
mod owner_tests;
mod process_tests;
mod project_tests;
mod service_tests;
mod store_tests;
