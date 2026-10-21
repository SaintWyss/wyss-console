//! F8-01 to F8-18 through the real application: a prepared application running its own
//! supervisor loop, clients on AF_UNIX socket pairs (the window's service and a raw protocol
//! client), SQLite V8, the engine, the scheduler and an owned child process per runtime.
//!
//! The child speaks the explicitly normalized test protocol and logs every frame it receives to
//! a file of its own, which is the oracle for "zero unauthorized provider frames". It is not
//! Codex and proves nothing about a provider: REAL_PROVIDER_ACCEPTED is separate and NOT_PROVEN.
//! Nothing here touches the installed authority, a credential, the network or the user's files.
use super::{
    authority::Authority,
    engine::{Engine, LaunchStage, Spawned, Spawner},
    gui::{F8Failure, F8Gui, FenceView, HandleConnector, SessionDetail},
    migration,
    ownership::{ContainmentPolicy, Grace},
    process_integration_tests::{AUTH, FixtureWorker},
    process_worker::DeferredProcessWorker,
    protocol::RuntimeRef,
    registry::Registry,
    service::Service,
    staging::{LaunchStaging, StagingRoot},
};
use crate::{
    contracts::{
        WireUuid,
        control::{
            ActualState, ApprovalDecision, ApprovalState, BindingState, ControlError as E,
            ControlResult, ExecutionScope, unique_json,
        },
        identities::ManagedSessionId,
    },
    control::{
        F8Transport,
        application::{ApplicationHandle, LocalSelection, PreparedApplication, run_selected},
        f8_fixture,
        framing::{read_frame, write_frame},
        store::uuid,
    },
    sessions::AgentMode,
};
use rusqlite::{
    Connection,
    hooks::{AuthAction, Authorization},
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    os::unix::{fs::DirBuilderExt, net::UnixStream},
    path::PathBuf,
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// The child. One process per runtime launch; every frame it is sent is appended to `log`.
const SCRIPT: &str = r#"
import json,os,signal,subprocess,sys,time
workspace,profile,metadata,behavior,log_path=sys.argv[1:6]
metadata=json.loads(metadata); behavior=json.loads(behavior)
log=open(log_path,'a',buffering=1)
open(log_path+'.pid','w').write(str(os.getpid()))
def emit(v):
    sys.stdout.write(json.dumps(v,separators=(',',':'))+'\n'); sys.stdout.flush()
if behavior.get('ignore_term'): signal.signal(signal.SIGTERM,signal.SIG_IGN)
if behavior.get('grandchild'):
    p=subprocess.Popen(['/usr/bin/sleep','600'])
    open(log_path+'.grandchild','w').write(str(p.pid))
emit(dict(kind='runtime_ready',observed_version='0.159.0',workspace=workspace,profile=profile,revision=1))
active=None
for line in sys.stdin:
    log.write(line if line.endswith('\n') else line+'\n')
    if line.startswith('approval '):
        _,item,decision=line.strip().split(' ')
        if decision=='accept' and behavior.get('crash_on_accept'): os._exit(9)
        time.sleep(float(behavior.get('approval_delay_ms',0))/1000)
        emit(dict(kind='approval_item_consumed',provider_item_id=item))
        if decision=='accept': emit(dict(kind='approval_item_confirmed',provider_item_id=item))
        continue
    v=json.loads(line)
    if v['method'] in ('thread/start','thread/resume'):
        if behavior.get('crash_on_thread'): os._exit(9)
        emit(dict(kind='binding_confirmed',operation_id=v['operation_id'],provider_thread_id=v.get('thread_id') or 'fixture-'+v['operation_id']))
    elif v['method']=='turn/start':
        active=v['turn']
        emit(dict(kind='turn_running',managed_turn_id=active,provider_turn_id='provider-'+active))
        emit(dict(kind='content',managed_turn_id=active,provider_turn_id='provider-'+active,text='fixture stream'))
        if behavior.get('flood'):
            for _ in range(int(behavior.get('flood_count',1))):
                emit(dict(kind='content',managed_turn_id=active,provider_turn_id='provider-'+active,text='x'*int(behavior['flood'])))
        if behavior.get('oversize'):
            emit(dict(kind='content',managed_turn_id=active,provider_turn_id='provider-'+active,text='x'*int(behavior['oversize'])))
        if v['input']=='approve2':
            for item,key in (('fixture-item','first'),('fixture-item-2','second')):
                emit(dict(kind='approval_required',approval_kind='command_execution',provider_item_id=item,action_fingerprint=metadata[key]['fingerprint'],structure=metadata[key]['structure'],action=dict(kind='command',command='printf fixture',cwd=workspace,parsed_actions=[])))
        if v['input']=='approve':
            emit(dict(kind='approval_required',approval_kind='command_execution',provider_item_id='fixture-item',action_fingerprint=metadata['fingerprint'],structure=metadata['structure'],action=dict(kind='command',command='printf fixture',cwd=workspace,parsed_actions=[])))
        if v['input']=='finish':
            emit(dict(kind='turn_completed',managed_turn_id=active,provider_turn_id='provider-'+active))
            active=None
    elif v['method']=='turn/interrupt' and active:
        emit(dict(kind='turn_interrupted',managed_turn_id=active))
        active=None
if behavior.get('ignore_eof'):
    while True: time.sleep(0.05)
"#;

/// What a test changes while the application runs. Everything else is the application's.
#[derive(Clone, Default)]
struct Knobs {
    /// Launches the spawner was asked for.
    spawns: Arc<AtomicUsize>,
    /// The behavior of the next launches, oldest first; none left means ordinary.
    queue: Arc<Mutex<VecDeque<Value>>>,
    /// The audit table refuses every insert while set: the disk of the authority is full.
    audit_full: Arc<AtomicBool>,
    /// The next spawn fails after staging.
    spawn_fails: Arc<AtomicBool>,
    /// How long a launch gets to stop before it is killed, in milliseconds.
    grace_ms: Arc<AtomicUsize>,
    /// The launch-stage fault fires once while set, then disarms itself.
    stage_fires: Arc<AtomicBool>,
    /// The next launch was asked for in Inference: its child is bare and reports that profile.
    inference: Arc<AtomicBool>,
}

struct AppSpawner {
    workspace: PathBuf,
    tools: bool,
    dir: PathBuf,
    knobs: Knobs,
}
impl Spawner for AppSpawner {
    fn descendant_containment(&self) -> bool {
        self.tools
    }
    fn spawn(
        &mut self,
        launch: &RuntimeRef,
        staging: Option<&mut LaunchStaging>,
    ) -> ControlResult<Spawned> {
        self.knobs.spawns.fetch_add(1, Ordering::AcqRel);
        if let Some(staging) = staging {
            // Each launch stages a catalogue of its own, named for it.
            staging.write_file(
                "catalogue.json",
                format!("{{\"launch\":\"{}\"}}", launch.runtime_instance_id.as_str()).as_bytes(),
            )?;
        }
        if self.knobs.spawn_fails.swap(false, Ordering::AcqRel) {
            return Err(E::ProviderRejected);
        }
        let behavior = self
            .knobs
            .queue
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| json!({}));
        let log = self.dir.join(format!(
            "frames-{}-{}",
            launch.managed_session_id.as_str(),
            launch.generation
        ));
        let action = super::operations::ProviderAction::Command {
            command: "printf fixture".into(),
            cwd: self.workspace.to_str().unwrap().into(),
            parsed_actions: vec![],
        };
        let (_, fingerprint, structure) = action.metadata("fixture-item")?;
        let (_, second_fingerprint, second_structure) = action.metadata("fixture-item-2")?;
        // A launch can be asked to run bare, whatever the application's profile: its child is
        // then a direct child with a real PID, and nothing contains its descendants.
        let tools = self.tools
            && behavior.get("direct") != Some(&json!(true))
            && !self.knobs.inference.swap(false, Ordering::AcqRel);
        let mut command = if tools {
            let mut command = Command::new("/usr/bin/bwrap");
            command.args([
                "--unshare-pid",
                "--die-with-parent",
                "--dev-bind",
                "/",
                "/",
                "--",
                "/usr/bin/python3",
            ]);
            command
        } else {
            Command::new("/usr/bin/python3")
        };
        command
            .args(["-u", "-c", SCRIPT])
            .arg(&self.workspace)
            .arg(if tools {
                "codex_local_tools_v1"
            } else {
                "codex_inference_v1"
            })
            .arg(
                json!({"fingerprint":fingerprint,"structure":structure,
                    "first":{"fingerprint":fingerprint,"structure":structure},
                    "second":{"fingerprint":second_fingerprint,"structure":second_structure}})
                .to_string(),
            )
            .arg(behavior.to_string())
            .arg(&log);
        let grace_ms = self.knobs.grace_ms.load(Ordering::Acquire).max(20) as u64;
        Ok(Spawned {
            worker: Box::new(FixtureWorker(DeferredProcessWorker::spawn(
                command,
                Grace {
                    eof: Duration::from_millis(grace_ms),
                    term: Duration::from_millis(grace_ms),
                    kill: Duration::from_millis(grace_ms * 5),
                },
                if tools {
                    ContainmentPolicy::DescendantsContained
                } else {
                    ContainmentPolicy::DirectChildOnly
                },
            )?)),
        })
    }
}

#[derive(Default)]
struct Options {
    limit: usize,
    tools: bool,
    staging: bool,
    stage_fault: Option<LaunchStage>,
    /// Seeds the V8 image before the application owns it.
    seed: Option<Box<dyn FnOnce(&Connection)>>,
    /// A durable, file-backed image opened the way the application opens one: with the
    /// production constructor, which validates it and recovers what a predecessor left.
    file: bool,
    /// The directory of an earlier application, whose image and logs this one continues.
    root: Option<PathBuf>,
    /// The identity of this authority instance, when it is not the fixture's.
    authority: Option<String>,
}

struct App {
    gui: F8Gui,
    handle: ApplicationHandle,
    knobs: Knobs,
    /// Workspace of the runtimes, and where their frame logs and staging live.
    dir: PathBuf,
    staging: PathBuf,
    /// Where the durable image lives, when it is a file.
    db: PathBuf,
    cleanup: bool,
    run: Option<JoinHandle<ControlResult<()>>>,
}
impl App {
    fn start(options: Options) -> Self {
        let dir = options.root.clone().unwrap_or_else(|| {
            std::env::temp_dir().join(format!("wyss-f8-accept-{}", uuid().as_str()))
        });
        let staging = dir.join("staging");
        let workspace = dir.join("workspace");
        for path in [&dir, &staging, &workspace] {
            if !path.exists() {
                std::fs::DirBuilder::new().mode(0o700).create(path).unwrap();
            }
        }
        let db = dir.join("authority.sqlite3");
        let knobs = Knobs::default();
        knobs.grace_ms.store(50, Ordering::Release);
        let connection = if options.file {
            if !db.exists() {
                let mut source = f8_fixture::v7_with_sessions(2);
                let seeded = migration::prepare(&mut source).unwrap().into_connection();
                seeded
                    .execute("VACUUM INTO ?1", [db.to_str().unwrap()])
                    .unwrap();
            }
            Connection::open(&db).unwrap()
        } else {
            let mut source = f8_fixture::v7_with_sessions(2);
            migration::prepare(&mut source).unwrap().into_connection()
        };
        if let Some(seed) = options.seed {
            seed(&connection);
        }
        let full = knobs.audit_full.clone();
        connection
            .authorizer(Some(
                move |ctx: rusqlite::hooks::AuthContext<'_>| match ctx.action {
                    AuthAction::Insert { table_name }
                        if table_name == "control_events" && full.load(Ordering::Acquire) =>
                    {
                        Authorization::Deny
                    }
                    _ => Authorization::Allow,
                },
            ))
            .unwrap();
        let authority =
            WireUuid::try_from(options.authority.clone().unwrap_or_else(|| AUTH.to_owned()))
                .unwrap();
        let spawner = AppSpawner {
            workspace: workspace.clone(),
            tools: options.tools,
            dir: dir.clone(),
            knobs: knobs.clone(),
        };
        if options.file {
            let (application, handle) = PreparedApplication::new(
                connection,
                authority,
                options
                    .staging
                    .then(|| StagingRoot::open(&staging).unwrap()),
                spawner,
            )
            .unwrap();
            let run = std::thread::spawn(move || {
                run_selected(LocalSelection::PreparedProtocol2(Box::new(application)))
            });
            return Self {
                gui: F8Gui::local(HandleConnector(handle.clone())),
                handle,
                knobs,
                dir: workspace,
                staging,
                db,
                cleanup: true,
                run: Some(run),
            };
        }
        let mut engine = Engine::new(
            connection,
            authority.clone(),
            Registry::new(authority)
                .unwrap()
                .with_test_limit(options.limit.max(1)),
            options
                .staging
                .then(|| StagingRoot::open(&staging).unwrap()),
        )
        .unwrap();
        if let Some(stage) = options.stage_fault {
            knobs.stage_fires.store(true, Ordering::Release);
            let fires = knobs.stage_fires.clone();
            engine = engine.with_stage_hook(move |at| {
                if at == stage && fires.swap(false, Ordering::AcqRel) {
                    Err(E::InternalError)
                } else {
                    Ok(())
                }
            });
        }
        let (application, handle) = PreparedApplication::with_test_transport(F8Transport::new(
            Service::new(Authority::new(engine), spawner),
        ));
        let run = std::thread::spawn(move || {
            run_selected(LocalSelection::PreparedProtocol2(Box::new(application)))
        });
        Self {
            gui: F8Gui::local(HandleConnector(handle.clone())),
            handle,
            knobs,
            dir: workspace,
            staging,
            db,
            cleanup: true,
            run: Some(run),
        }
    }
    /// Ends the application as its owner does and answers where its directory is, for the
    /// next application to continue from. Nothing is deleted.
    fn retire(mut self) -> PathBuf {
        self.cleanup = false;
        let result = self.stop();
        assert_eq!(result, Ok(()), "an application ends cleanly");
        self.dir.parent().unwrap().to_owned()
    }
    fn workspace(&self) -> String {
        self.dir.to_str().unwrap().to_owned()
    }
    /// The frames the runtime of one launch was sent, one per line.
    fn frames(&self, session: &ManagedSessionId, generation: u64) -> Vec<String> {
        let path = self
            .dir
            .parent()
            .unwrap()
            .join(format!("frames-{}-{generation}", session.as_str()));
        std::fs::read_to_string(path)
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }
    /// How many frames of one method a launch was sent.
    fn sent(&self, session: &ManagedSessionId, generation: u64, method: &str) -> usize {
        self.frames(session, generation)
            .iter()
            .filter(|line| {
                if method == "approval" {
                    line.starts_with("approval ")
                } else {
                    serde_json::from_str::<Value>(line).is_ok_and(|v| v["method"] == method)
                }
            })
            .count()
    }
    fn pid_file(&self, session: &ManagedSessionId, generation: u64, suffix: &str) -> Option<u32> {
        std::fs::read_to_string(
            self.dir
                .parent()
                .unwrap()
                .join(format!("frames-{}-{generation}.{suffix}", session.as_str())),
        )
        .ok()
        .and_then(|text| text.trim().parse().ok())
    }
    fn raw(&self) -> Raw {
        Raw::connect(&self.handle)
    }
    fn stop(&mut self) -> ControlResult<()> {
        self.gui.shutdown();
        self.handle.shutdown();
        self.run.take().map_or(Ok(()), |run| run.join().unwrap())
    }
}
impl Drop for App {
    fn drop(&mut self) {
        let _ = self.stop();
        if self.cleanup {
            let _ = std::fs::remove_dir_all(self.dir.parent().unwrap());
        }
    }
}

/// A raw protocol-2 client: every byte it sends is its own, so it can say what the window
/// never would.
struct Raw {
    stream: UnixStream,
    authority: Option<String>,
    stop: AtomicBool,
    /// Every method this client sent, and whether the authority answered it with a result.
    log: Arc<Mutex<Vec<(String, bool)>>>,
}
impl Raw {
    fn connect(handle: &ApplicationHandle) -> Self {
        let (owned, client) = UnixStream::pair().unwrap();
        client.set_nonblocking(true).unwrap();
        handle.attach(owned).unwrap();
        Self {
            stream: client,
            authority: None,
            stop: AtomicBool::new(false),
            log: Arc::default(),
        }
    }
    fn exchange(&mut self, bytes: &[u8]) -> Value {
        let deadline = Instant::now() + Duration::from_secs(5);
        write_frame(&mut self.stream, bytes, deadline, &self.stop).unwrap();
        let answer = read_frame(&mut self.stream, deadline, &self.stop)
            .unwrap()
            .expect("the authority answered");
        unique_json(&answer).unwrap()
    }
    fn hello(&mut self) -> Value {
        let answer = self.send(
            "authority/hello",
            json!({"protocol_version":2,"client_instance_id":uuid().as_str()}),
        );
        self.authority = answer["authority_instance_id"].as_str().map(str::to_owned);
        answer
    }
    /// A request with its envelope; the authority is named when a hello has told it.
    fn send(&mut self, method: &str, params: Value) -> Value {
        let mut request = json!({"schema_version":2,"request_id":uuid().as_str(),"method":method,"params":params});
        if method != "authority/hello" {
            // Before a hello there is no authority to name: any identity is as good as none.
            request["authority_instance_id"] = json!(
                self.authority
                    .clone()
                    .unwrap_or_else(|| uuid().as_str().to_owned())
            );
        }
        let answer = self.exchange(&serde_json::to_vec(&request).unwrap());
        self.log
            .lock()
            .unwrap()
            .push((method.to_owned(), answer["error"].is_null()));
        answer
    }
    fn code(answer: &Value) -> Option<&str> {
        answer["error"]["code"].as_str()
    }
}

/// A fence as the protocol spells it: the window's name for it is not the wire's.
fn wire(fence: &FenceView) -> Value {
    json!({
        "managed_session_id": fence.session,
        "generation": fence.generation,
        "runtime_instance_id": fence.instance,
    })
}

fn eventually<T>(mut read: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(value) = read() {
            return value;
        }
        assert!(Instant::now() < until, "acceptance deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}
/// Asks again, with the same identity, while the authority is still working on it.
fn settled<T>(mut ask: impl FnMut() -> Result<T, super::gui::F8Error>) -> T {
    eventually(|| match ask() {
        Ok(value) => Some(value),
        Err(e) if e.code == F8Failure::Busy => None,
        Err(e) => panic!("{e:?}"),
    })
}

const ALL: [ExecutionScope; 5] = [
    ExecutionScope::SessionCreate,
    ExecutionScope::SessionResume,
    ExecutionScope::TurnStart,
    ExecutionScope::CommandExecute,
    ExecutionScope::FileChange,
];

/// A session with a ready runtime, an active lease and an attached binding.
struct Session {
    id: ManagedSessionId,
    fence: FenceView,
    detail: SessionDetail,
}
impl App {
    fn session(&self) -> ManagedSessionId {
        self.gui.prepare(uuid()).unwrap().id
    }
    fn ready(&self, id: &ManagedSessionId, mode: AgentMode) -> SessionDetail {
        let row = self.gui.session(id.clone()).unwrap().session;
        self.knobs
            .inference
            .store(mode == AgentMode::Inference, Ordering::Release);
        self.gui
            .runtime_start(
                uuid(),
                id.clone(),
                row.runtime.revision,
                row.runtime.generation,
                mode,
                (mode == AgentMode::LocalTools).then(|| self.workspace()),
            )
            .unwrap();
        eventually(|| {
            let d = self.gui.session(id.clone()).ok()?;
            (d.session.runtime.actual == ActualState::Ready).then_some(d)
        })
    }
    /// Runtime, lease and binding: the state in which a turn can start.
    fn up(&self, mode: AgentMode) -> Session {
        let id = self.session();
        let ready = self.ready(&id, mode);
        let fence = ready.session.runtime.fence.clone();
        self.gui
            .lease_grant(
                uuid(),
                fence.clone(),
                ready.session.revision,
                ALL.to_vec(),
                300,
            )
            .unwrap();
        let intent = uuid();
        settled(|| {
            self.gui.binding_create(
                intent.clone(),
                fence.clone(),
                ready.session.revision,
                self.workspace(),
            )
        });
        let detail = eventually(|| {
            let d = self.gui.session(id.clone()).ok()?;
            (d.binding.state == BindingState::BoundAttached).then_some(d)
        });
        Session { id, fence, detail }
    }
    /// A turn whose prompt makes the child propose a command, and the approval it waits on.
    fn waiting(
        &self,
        s: &Session,
    ) -> (
        crate::contracts::identities::ManagedTurnId,
        crate::turns::ApprovalDetail,
    ) {
        let intent = uuid();
        let turn = settled(|| {
            self.gui.turn_start(
                intent.clone(),
                s.fence.clone(),
                s.detail.session.revision,
                "approve".into(),
            )
        })
        .id;
        let notice = eventually(|| self.gui.turn(turn.clone(), None, true).ok()?.approval);
        (turn, self.gui.approval(notice.id, None, None).unwrap())
    }
    fn decide(
        &self,
        s: &Session,
        approval: &crate::turns::ApprovalDetail,
        decision: ApprovalDecision,
    ) -> Result<crate::f8::gui::DecisionReceipt, super::gui::F8Error> {
        self.gui.approval_respond(
            uuid(),
            s.fence.clone(),
            approval.approval.id.clone(),
            approval.approval.revision,
            decision,
        )
    }
}

// ---------------------------------------------------------------------------------- F8-12

#[test]
fn f8_12_version_confusion_never_selects_an_implicit_session() {
    let app = App::start(Options {
        limit: 1,
        ..Options::default()
    });
    let a = app.session();
    // A protocol-1 hello is refused: there is no fallback.
    let mut raw = app.raw();
    let v1 = raw.exchange(
        &serde_json::to_vec(&json!({"schema_version":1,"request_id":uuid().as_str(),
            "method":"authority/hello","params":{"protocol_version":1,"client_instance_id":uuid().as_str()}}))
        .unwrap(),
    );
    assert!(Raw::code(&v1).is_some(), "{v1}");
    // Nothing else is answered before a hello.
    let early = raw.send("authority/status", json!({}));
    assert_eq!(Raw::code(&early), Some("handshake_required"), "{early}");
    let hello = raw.hello();
    assert_eq!(hello["result"]["value"]["protocol_version"], 2);
    assert_eq!(hello["result"]["value"]["storage_schema_version"], 8);
    // A second hello, an unknown schema and an unknown method are refused.
    assert!(Raw::code(&raw.hello()).is_some());
    let other = raw.exchange(
        &serde_json::to_vec(&json!({"schema_version":3,"request_id":uuid().as_str(),
            "authority_instance_id":raw.authority,"method":"authority/status","params":{}}))
        .unwrap(),
    );
    assert!(Raw::code(&other).is_some(), "{other}");
    let unknown = raw.send("runtime/everything", json!({}));
    assert_eq!(Raw::code(&unknown), Some("unsupported_method"), "{unknown}");
    // A request that omits the session never falls back on the only session or the newest.
    for (method, params) in [
        ("runtime/status", json!({})),
        (
            "runtime/start",
            json!({"intent_id":uuid().as_str(),"adapter_id":"codex","runtime_kind":"codex","expected_revision":0}),
        ),
        ("managedSession/get", json!({})),
    ] {
        let refused = raw.send(method, params);
        assert_eq!(
            Raw::code(&refused),
            Some("invalid_request"),
            "{method} {refused}"
        );
    }
    // The authority named in a request is the one the hello named: another one is refused,
    // and so is a session that belongs to nobody.
    let mut stale = app.raw();
    stale.hello();
    stale.authority = Some(uuid().as_str().to_owned());
    let wrong = stale.send("authority/status", json!({}));
    assert_eq!(Raw::code(&wrong), Some("correlation_error"), "{wrong}");
    let missing = raw.send(
        "runtime/status",
        json!({"adapter_id":"codex","runtime_kind":"codex","managed_session_id":uuid().as_str()}),
    );
    assert_eq!(Raw::code(&missing), Some("session_not_found"), "{missing}");
    // The one session that exists was never touched by any of it.
    assert_eq!(app.gui.session(a).unwrap().session.runtime.generation, 0);
    assert_eq!(app.knobs.spawns.load(Ordering::Acquire), 0);
}

// ---------------------------------------------------------------------------------- F8-01

#[test]
fn f8_01_a_lease_used_with_bs_runtime_attachment_or_turn_is_refused_before_dispatch() {
    let app = App::start(Options {
        limit: 2,
        tools: true,
        ..Options::default()
    });
    let (a, b) = (app.up(AgentMode::LocalTools), app.up(AgentMode::LocalTools));
    let b_before = app.gui.session(b.id.clone()).unwrap();
    let frames_b = app.frames(&b.id, 1);
    let mut raw = app.raw();
    raw.hello();
    // A's fence cannot drive B's session, whichever field names B.
    let cross = raw.send(
        "managedSession/turn/start",
        json!({
        "intent_id":uuid().as_str(),"managed_session_id":b.id.as_str(),
        "expected_session_revision":1,"prompt":"work","runtime_fence":wire(&a.fence)}),
    );
    assert_eq!(Raw::code(&cross), Some("invalid_request"), "{cross}");
    let lease_a = app.gui.session(a.id.clone()).unwrap().lease.unwrap();
    // A's lease, named with B's fence, renews nothing.
    let renew = raw.send(
        "executionLease/renew",
        json!({
        "intent_id":uuid().as_str(),"lease_id":lease_a.id.as_str(),
        "expected_revision":lease_a.revision,"ttl_seconds":60,"runtime_fence":wire(&b.fence)}),
    );
    assert!(
        matches!(Raw::code(&renew), Some("gate_denied" | "invalid_request")),
        "{renew}"
    );
    // B's own lease with A's session in the grant is a conflict of identities, not a grant.
    let grant = raw.send(
        "executionLease/grant",
        json!({
        "intent_id":uuid().as_str(),"session_id":b.id.as_str(),"expected_session_revision":1,
        "scopes":["turn_start"],"ttl_seconds":60,"runtime_fence":wire(&a.fence)}),
    );
    assert_eq!(Raw::code(&grant), Some("invalid_request"), "{grant}");
    // A's approval cannot be answered through B's fence, and a binding cannot be created
    // for B under A's.
    let (turn_a, approval_a) = app.waiting(&a);
    let answer = raw.send("managedTurn/approval/respond", json!({
        "intent_id":uuid().as_str(),"managed_approval_id":approval_a.approval.id.as_str(),
        "expected_revision":approval_a.approval.revision,"decision":"accept","runtime_fence":wire(&b.fence)}));
    let crossed = ["gate_denied", "stale_revision", "invalid_request"];
    assert_refused(&answer, &crossed);
    // ... nor can a binding be created for B under A's.
    let bind = raw.send(
        "managedSession/runtime/create",
        json!({"intent_id":uuid().as_str(),"managed_session_id":b.id.as_str(),
            "expected_session_revision":1,"workspace_root":app.workspace(),
            "runtime_fence":wire(&a.fence)}),
    );
    assert_refused(&bind, &crossed);
    let cancel = raw.send(
        "managedSession/turn/cancel",
        json!({
        "intent_id":uuid().as_str(),"managed_turn_id":turn_a.as_str(),
        "expected_revision":2,"runtime_fence":wire(&b.fence)}),
    );
    assert_refused(&cancel, &crossed);
    // Zero frames reached B; its rows are those it had; A is still waiting.
    assert_eq!(app.frames(&b.id, 1), frames_b);
    assert_eq!(app.sent(&b.id, 1, "turn/start"), 0);
    assert_eq!(app.sent(&a.id, 1, "approval"), 0);
    let b_after = app.gui.session(b.id.clone()).unwrap();
    assert_eq!(b_after.session.revision, b_before.session.revision);
    assert_eq!(
        b_after.lease.map(|l| l.revision),
        b_before.lease.map(|l| l.revision)
    );
    assert!(b_after.turns.is_empty());
    assert!(
        app.gui
            .approval(approval_a.approval.id.clone(), None, None)
            .unwrap()
            .approval
            .decidable
    );
    drop(turn_a);
}

// ---------------------------------------------------------------------------------- F8-02

#[test]
fn f8_02_identical_provider_item_ids_never_cross_between_runtimes() {
    let app = App::start(Options {
        limit: 2,
        tools: true,
        ..Options::default()
    });
    let (a, b) = (app.up(AgentMode::LocalTools), app.up(AgentMode::LocalTools));
    // Both children propose an item with the very same provider id.
    let (_, approval_a) = app.waiting(&a);
    let (turn_b, approval_b) = app.waiting(&b);
    assert_ne!(approval_a.approval.id, approval_b.approval.id);
    let receipt = app.decide(&a, &approval_a, ApprovalDecision::Accept);
    let receipt = receipt
        .or_else(|e| {
            if e.code == F8Failure::Busy {
                app.decide(&a, &approval_a, ApprovalDecision::Accept)
            } else {
                Err(e)
            }
        })
        .unwrap();
    assert_eq!(receipt.state, ApprovalState::Accepted);
    // Only A's child was told; B's approval is as it was.
    eventually(|| (app.sent(&a.id, 1, "approval") == 1).then_some(()));
    assert_eq!(app.sent(&b.id, 1, "approval"), 0);
    let still = app
        .gui
        .approval(approval_b.approval.id.clone(), None, None)
        .unwrap();
    assert!(still.approval.decidable);
    assert_eq!(still.approval.revision, approval_b.approval.revision);
    // B's own decision reaches B's child, and only it.
    let settled_b = settled(|| app.decide(&b, &approval_b, ApprovalDecision::Decline));
    assert_eq!(settled_b.state, ApprovalState::Declined);
    eventually(|| (app.sent(&b.id, 1, "approval") == 1).then_some(()));
    assert_eq!(app.sent(&a.id, 1, "approval"), 1);
    drop(turn_b);
}

// ---------------------------------------------------------------------------------- helpers

impl App {
    /// The detail of a session, read until `done` holds.
    fn until(&self, id: &ManagedSessionId, done: impl Fn(&SessionDetail) -> bool) -> SessionDetail {
        eventually(|| {
            let d = self.gui.session(id.clone()).ok()?;
            done(&d).then_some(d)
        })
    }
    /// Ends a session's turn and then its runtime, the way the user does.
    fn wind_down(
        &self,
        s: &Session,
        turn: &crate::contracts::identities::ManagedTurnId,
    ) -> SessionDetail {
        let reading = self.gui.turn(turn.clone(), None, false).unwrap();
        let cancel = uuid();
        settled(|| {
            self.gui.turn_cancel(
                cancel.clone(),
                s.fence.clone(),
                turn.clone(),
                reading.turn.turn.revision,
            )
        });
        eventually(|| {
            self.gui
                .turn(turn.clone(), None, false)
                .ok()
                .filter(|r| r.turn.turn.state.terminal())
        });
        let now = self.gui.session(s.id.clone()).unwrap();
        self.gui
            .runtime_stop(
                uuid(),
                now.session.runtime.fence.clone(),
                now.session.runtime.revision,
            )
            .unwrap();
        self.until(&s.id, |d| d.session.runtime.actual == ActualState::Stopped)
    }
    /// Starts the runtime of a session again, as a new launch.
    fn relaunch(&self, id: &ManagedSessionId, mode: AgentMode) -> SessionDetail {
        let row = self.gui.session(id.clone()).unwrap().session;
        self.knobs
            .inference
            .store(mode == AgentMode::Inference, Ordering::Release);
        self.gui
            .runtime_start(
                uuid(),
                id.clone(),
                row.runtime.revision,
                row.runtime.generation,
                mode,
                (mode == AgentMode::LocalTools).then(|| self.workspace()),
            )
            .unwrap();
        self.until(id, |d| {
            d.session.runtime.actual == ActualState::Ready
                && d.session.runtime.generation > row.runtime.generation
        })
    }
    /// The durable head of the stream, as the authority reports it.
    fn head(&self) -> Option<String> {
        self.gui
            .state()
            .authority
            .and_then(|a| a.last_sequence)
            .map(|s| s.get().to_string())
    }
    /// Events of one kind in the durable history, by reading the whole stream.
    fn events(&self, kind: &str) -> usize {
        let mut older: Option<(WireUuid, crate::contracts::Sequence)> = None;
        let mut count = 0;
        loop {
            let page = match older.take() {
                None => self.gui.audit_latest().unwrap(),
                Some((stream, before)) => self.gui.audit_older(stream, before).unwrap(),
            };
            count += page
                .page
                .events
                .iter()
                .filter(|e| serde_json::to_value(e.kind).unwrap() == kind)
                .count();
            match page.page.older {
                Some(before) => older = Some((page.page.stream.clone(), before)),
                None => return count,
            }
        }
    }
}
fn assert_refused(answer: &Value, allowed: &[&str]) {
    let code = Raw::code(answer);
    assert!(
        code.is_some_and(|c| allowed.contains(&c)),
        "expected one of {allowed:?}: {answer}"
    );
}
fn alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}/stat")).exists()
}

// ---------------------------------------------------------------------------------- F8-03

#[test]
fn f8_03_traffic_of_an_earlier_generation_is_refused_after_a_relaunch() {
    let app = App::start(Options {
        limit: 1,
        tools: true,
        ..Options::default()
    });
    let s = app.up(AgentMode::LocalTools);
    let (turn, approval) = app.waiting(&s);
    app.wind_down(&s, &turn);
    let second = app.relaunch(&s.id, AgentMode::LocalTools);
    assert_eq!(second.session.runtime.generation, 2);
    assert_ne!(second.session.runtime.instance, s.fence.instance);
    let before = (
        second.session.runtime.revision,
        second.lease.map(|l| l.revision),
        app.head(),
    );
    let mut raw = app.raw();
    raw.hello();
    let session = s.id.as_str();
    let gone = s.fence.clone();
    let stale = [
        "stale_revision",
        "invalid_transition",
        "gate_denied",
        "approval_not_found",
    ];
    for (method, params) in [
        (
            "managedSession/turn/start",
            json!({"intent_id":uuid().as_str(),"managed_session_id":session,"expected_session_revision":1,"prompt":"again","runtime_fence":wire(&gone)}),
        ),
        (
            "executionLease/grant",
            json!({"intent_id":uuid().as_str(),"session_id":session,"expected_session_revision":1,"scopes":["turn_start"],"ttl_seconds":60,"runtime_fence":wire(&gone)}),
        ),
        (
            "managedSession/runtime/resume",
            json!({"intent_id":uuid().as_str(),"managed_session_id":session,"expected_session_revision":1,"runtime_fence":wire(&gone)}),
        ),
        (
            "managedSession/runtime/create",
            json!({"intent_id":uuid().as_str(),"managed_session_id":session,"expected_session_revision":1,"workspace_root":app.workspace(),"runtime_fence":wire(&gone)}),
        ),
        (
            "managedTurn/approval/respond",
            json!({"intent_id":uuid().as_str(),"managed_approval_id":approval.approval.id.as_str(),"expected_revision":approval.approval.revision,"decision":"accept","runtime_fence":wire(&gone)}),
        ),
        (
            "runtime/stop",
            json!({"intent_id":uuid().as_str(),"adapter_id":"codex","runtime_kind":"codex","managed_session_id":session,"expected_revision":second.session.runtime.revision,"expected_generation":1,"expected_runtime_instance_id":gone.instance}),
        ),
        (
            "runtime/model/list",
            json!({"adapter_id":"codex","runtime_kind":"codex","managed_session_id":session,"expected_generation":1,"expected_runtime_instance_id":gone.instance,"cursor":null,"limit":20}),
        ),
        (
            "managedSession/close",
            json!({"intent_id":uuid().as_str(),"managed_session_id":session,"expected_revision":1,"runtime_fence":wire(&gone)}),
        ),
    ] {
        let answer = raw.send(method, params);
        assert!(
            Raw::code(&answer).is_some_and(|c| stale.contains(&c)),
            "{method}: {answer}"
        );
    }
    // A cancel of a turn that already ended returns what the authority recorded for it, read
    // from the launch it ran on; it neither finds generation one alive nor writes to two.
    let cancel = raw.send(
        "managedSession/turn/cancel",
        json!({"intent_id":uuid().as_str(),"managed_turn_id":turn.as_str(),
            "expected_revision":3,"runtime_fence":wire(&gone)}),
    );
    assert_eq!(
        cancel["result"]["value"]["state"], "interrupted",
        "{cancel}"
    );
    assert_eq!(cancel["result"]["value"]["managed_turn_id"], turn.as_str());
    // Nothing of generation one reached generation two: its child heard nothing, and the
    // authority's rows are those they were.
    assert!(
        app.frames(&s.id, 2).is_empty(),
        "{:?}",
        app.frames(&s.id, 2)
    );
    assert_eq!(app.knobs.spawns.load(Ordering::Acquire), 2);
    let after = app.gui.session(s.id.clone()).unwrap();
    assert_eq!(
        (
            after.session.runtime.revision,
            after.lease.map(|l| l.revision),
            app.head()
        ),
        before
    );
    assert_eq!(after.session.runtime.generation, 2);
}

// ---------------------------------------------------------------------------------- F8-04

#[test]
fn f8_04_an_unknown_outcome_at_a_crash_is_never_retried_through_a_new_generation() {
    let app = App::start(Options {
        limit: 1,
        tools: true,
        ..Options::default()
    });
    app.knobs
        .queue
        .lock()
        .unwrap()
        .push_back(json!({"crash_on_accept": true}));
    let s = app.up(AgentMode::LocalTools);
    let (turn, approval) = app.waiting(&s);
    // The child dies as the accepted answer reaches it: whether the action ran is unknown.
    let decision = uuid();
    let outcome = eventually(|| {
        match app.gui.approval_respond(
            decision.clone(),
            s.fence.clone(),
            approval.approval.id.clone(),
            approval.approval.revision,
            ApprovalDecision::Accept,
        ) {
            Err(e) if e.code == F8Failure::Busy => None,
            other => Some(other),
        }
    });
    let _ = outcome;
    let failed = app.until(&s.id, |d| d.session.runtime.actual == ActualState::Failed);
    assert_eq!(
        failed.session.runtime.diagnostic.map(|d| format!("{d:?}")),
        Some("ChildExited".into())
    );
    let unknown = eventually(|| {
        let a = app
            .gui
            .approval(approval.approval.id.clone(), None, None)
            .ok()?;
        (a.approval.state != ApprovalState::Pending).then_some(a)
    });
    assert!(
        matches!(
            unknown.approval.state,
            ApprovalState::Indeterminate | ApprovalState::Accepted
        ),
        "{:?}",
        unknown.approval.state
    );
    assert!(!unknown.approval.decidable);
    let ended = app.gui.turn(turn.clone(), None, false).unwrap();
    assert!(ended.turn.turn.state.terminal());
    // A new generation starts, and the unknown stays unknown: nothing is sent again.
    let second = app.relaunch(&s.id, AgentMode::LocalTools);
    assert_eq!(second.session.runtime.generation, 2);
    let still = app
        .gui
        .approval(approval.approval.id.clone(), None, None)
        .unwrap();
    assert_eq!(still.approval.state, unknown.approval.state);
    assert!(!still.approval.decidable);
    assert_eq!(app.sent(&s.id, 2, "approval"), 0);
    assert!(app.frames(&s.id, 2).is_empty());
    // Asking again, even with the identity it had, does not run it again.
    let again = app.gui.approval_respond(
        decision.clone(),
        s.fence.clone(),
        approval.approval.id.clone(),
        approval.approval.revision,
        ApprovalDecision::Accept,
    );
    let _ = again;
    assert_eq!(app.sent(&s.id, 2, "approval"), 0);
    assert_eq!(app.sent(&s.id, 1, "approval"), 1);
}

// ---------------------------------------------------------------------------------- F8-05

#[test]
fn f8_05_a_lease_revoked_or_expired_after_admission_writes_nothing_while_b_continues() {
    let app = App::start(Options {
        limit: 2,
        tools: true,
        ..Options::default()
    });
    let (a, b) = (app.up(AgentMode::LocalTools), app.up(AgentMode::LocalTools));
    let (_, approval_a) = app.waiting(&a);
    let (_, approval_b) = app.waiting(&b);
    let lease_a = app.gui.session(a.id.clone()).unwrap().lease.unwrap();
    app.gui
        .lease_revoke(uuid(), lease_a.id.clone(), lease_a.revision)
        .unwrap();
    // A's approval ended with its lease, in the same commit; answering it writes nothing.
    let ended = app
        .gui
        .approval(approval_a.approval.id.clone(), None, None)
        .unwrap();
    assert_ne!(ended.approval.state, ApprovalState::Pending);
    let refused = app.decide(&a, &approval_a, ApprovalDecision::Accept);
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(app.sent(&a.id, 1, "approval"), 0);
    // B is as admitted as it was: its answer reaches its child.
    let accepted = settled(|| app.decide(&b, &approval_b, ApprovalDecision::Accept));
    assert_eq!(accepted.state, ApprovalState::Accepted);
    eventually(|| (app.sent(&b.id, 1, "approval") == 1).then_some(()));
    assert!(app.gui.session(b.id.clone()).unwrap().lease.is_some());
}

#[test]
fn f8_05_a_lease_that_expires_after_admission_ends_the_approval_it_admitted() {
    let app = App::start(Options {
        limit: 1,
        tools: true,
        ..Options::default()
    });
    let id = app.session();
    let ready = app.ready(&id, AgentMode::LocalTools);
    let fence = ready.session.runtime.fence.clone();
    app.gui
        .lease_grant(
            uuid(),
            fence.clone(),
            ready.session.revision,
            ALL.to_vec(),
            3,
        )
        .unwrap();
    let intent = uuid();
    settled(|| {
        app.gui.binding_create(
            intent.clone(),
            fence.clone(),
            ready.session.revision,
            app.workspace(),
        )
    });
    let detail = app.until(&id, |d| d.binding.state == BindingState::BoundAttached);
    let s = Session {
        id: id.clone(),
        fence,
        detail,
    };
    let (_, approval) = app.waiting(&s);
    std::thread::sleep(Duration::from_millis(3500));
    let late = app.decide(&s, &approval, ApprovalDecision::Accept);
    assert!(late.is_err(), "{late:?}");
    assert_eq!(app.sent(&id, 1, "approval"), 0);
    let ended = app
        .gui
        .approval(approval.approval.id.clone(), None, None)
        .unwrap();
    assert!(!ended.approval.decidable);
    assert_ne!(ended.approval.state, ApprovalState::Accepted);
}

// ---------------------------------------------------------------------------------- F8-06

#[test]
fn f8_06_replaying_old_intents_after_runtime_loss_returns_the_original_receipt_and_revives_nothing()
{
    let app = App::start(Options {
        limit: 1,
        tools: true,
        ..Options::default()
    });
    let id = app.session();
    let mut raw = app.raw();
    raw.hello();
    let workspace = app.workspace();
    // Every request below is built once and kept: a replay is these very bytes again.
    let start = json!({"intent_id":uuid().as_str(),"adapter_id":"codex","runtime_kind":"codex",
        "managed_session_id":id.as_str(),"expected_revision":0,"expected_generation":0,
        "safety_profile":"codex_local_tools_v1","workspace_root":workspace});
    let started = raw.send("runtime/start", start.clone());
    assert!(started["error"].is_null(), "{started}");
    let ready = app.until(&id, |d| d.session.runtime.actual == ActualState::Ready);
    let fence = wire(&ready.session.runtime.fence);
    let grant = json!({"intent_id":uuid().as_str(),"session_id":id.as_str(),
        "expected_session_revision":1,"scopes":["session_create","session_resume","turn_start","command_execute","file_change"],
        "ttl_seconds":300,"runtime_fence":fence});
    let lease = raw.send("executionLease/grant", grant.clone());
    assert!(lease["error"].is_null(), "{lease}");
    let create = json!({"intent_id":uuid().as_str(),"managed_session_id":id.as_str(),
        "expected_session_revision":1,"workspace_root":workspace,"runtime_fence":fence});
    let bound = eventually(|| {
        let r = raw.send("managedSession/runtime/create", create.clone());
        (Raw::code(&r) != Some("runtime_operation_in_progress")).then_some(r)
    });
    assert!(bound["error"].is_null(), "{bound}");
    app.until(&id, |d| d.binding.state == BindingState::BoundAttached);
    let turn = json!({"intent_id":uuid().as_str(),"managed_session_id":id.as_str(),
        "expected_session_revision":1,"prompt":"approve","runtime_fence":fence});
    let admitted = eventually(|| {
        let r = raw.send("managedSession/turn/start", turn.clone());
        (Raw::code(&r) != Some("runtime_operation_in_progress")).then_some(r)
    });
    assert!(admitted["error"].is_null(), "{admitted}");
    let turn_id = admitted["result"]["value"]["managed_turn_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let approval = eventually(|| {
        let r = raw.send(
            "managedTurn/approval/pending",
            json!({"managed_turn_id":turn_id}),
        );
        r["result"]["value"]["approval"]
            .as_object()
            .map(|_| r.clone())
    });
    let approval_id = approval["result"]["value"]["approval"]["managed_approval_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let respond = json!({"intent_id":uuid().as_str(),"managed_approval_id":approval_id,
        "expected_revision":1,"decision":"accept","runtime_fence":fence});
    let decided = eventually(|| {
        let r = raw.send("managedTurn/approval/respond", respond.clone());
        (Raw::code(&r) != Some("runtime_operation_in_progress")).then_some(r)
    });
    assert!(decided["error"].is_null(), "{decided}");
    eventually(|| (app.sent(&id, 1, "approval") == 1).then_some(()));

    // The runtime is lost: revoking the lease ends the turn it admitted in the same commit, and
    // the runtime is then stopped. There is nothing left for an old intent to revive.
    let tid = crate::contracts::identities::ManagedTurnId::parse(&turn_id).unwrap();
    let current = app.gui.session(id.clone()).unwrap();
    let lease_now = current.lease.clone().unwrap();
    app.gui
        .lease_revoke(uuid(), lease_now.id.clone(), lease_now.revision)
        .unwrap();
    eventually(|| {
        app.gui
            .turn(tid.clone(), None, false)
            .ok()
            .filter(|r| r.turn.turn.state.terminal())
    });
    let after = app.gui.session(id.clone()).unwrap();
    app.gui
        .runtime_stop(
            uuid(),
            after.session.runtime.fence.clone(),
            after.session.runtime.revision,
        )
        .unwrap();
    app.until(&id, |d| d.session.runtime.actual == ActualState::Stopped);

    let (spawns, head, generation) = (
        app.knobs.spawns.load(Ordering::Acquire),
        app.head(),
        app.gui
            .session(id.clone())
            .unwrap()
            .session
            .runtime
            .generation,
    );
    let frames = app.frames(&id, 1);
    // Replay each, byte for byte: the original receipt comes back, and nothing happens.
    for (method, request, original) in [
        ("runtime/start", &start, &started),
        ("executionLease/grant", &grant, &lease),
        ("managedSession/runtime/create", &create, &bound),
        ("managedSession/turn/start", &turn, &admitted),
        ("managedTurn/approval/respond", &respond, &decided),
    ] {
        let again = raw.send(method, request.clone());
        assert_eq!(again["result"], original["result"], "{method}: {again}");
    }
    assert_eq!(
        app.knobs.spawns.load(Ordering::Acquire),
        spawns,
        "no duplicate launch"
    );
    assert_eq!(app.head(), head, "no new durable event");
    assert_eq!(app.frames(&id, 1), frames, "no frame to the old child");
    assert!(app.frames(&id, 2).is_empty());
    let end = app.gui.session(id.clone()).unwrap();
    assert_eq!(end.session.runtime.generation, generation);
    assert_eq!(end.session.runtime.actual, ActualState::Stopped);
    // The lease the old intent granted stays revoked: it is not revived, and new work needs a
    // fresh one.
    assert!(end.lease.is_none());
    let fresh = app.relaunch(&id, AgentMode::LocalTools);
    let new_turn = app.gui.turn_start(
        uuid(),
        fresh.session.runtime.fence.clone(),
        fresh.session.revision,
        "work".into(),
    );
    assert!(new_turn.is_err(), "{new_turn:?}");
    assert!(app.frames(&id, 2).is_empty());
}

// ---------------------------------------------------------------------------------- F8-07

fn start_params(session: &ManagedSessionId) -> Value {
    json!({"intent_id":uuid().as_str(),"adapter_id":"codex","runtime_kind":"codex",
        "managed_session_id":session.as_str(),"expected_revision":0,"expected_generation":0})
}

#[test]
fn f8_07_capacity_is_a_fixed_immediate_refusal_including_stopping_slots() {
    // One runtime at a time is the operating capacity, not a test knob.
    let app = App::start(Options {
        limit: 1,
        ..Options::default()
    });
    let (a, b, c) = (app.session(), app.session(), app.session());
    // The first launch ignores EOF and TERM, so its stop takes a while.
    app.knobs
        .queue
        .lock()
        .unwrap()
        .extend([json!({"ignore_term":true,"ignore_eof":true}), json!({})]);
    app.knobs.grace_ms.store(400, Ordering::Release);
    let mut first = app.raw();
    first.hello();
    let mut second = app.raw();
    second.hello();
    // Parallel admission exactly at the bound: one of two simultaneous starts wins.
    let gate = std::sync::Barrier::new(2);
    let (pa, pb) = (start_params(&a), start_params(&b));
    let answers: Vec<Value> = std::thread::scope(|scope| {
        let one = scope.spawn(|| {
            gate.wait();
            first.send("runtime/start", pa)
        });
        let two = scope.spawn(|| {
            gate.wait();
            second.send("runtime/start", pb)
        });
        vec![one.join().unwrap(), two.join().unwrap()]
    });
    let won: Vec<_> = answers.iter().filter(|r| r["error"].is_null()).collect();
    let refused: Vec<_> = answers.iter().filter(|r| !r["error"].is_null()).collect();
    assert_eq!((won.len(), refused.len()), (1, 1), "{answers:?}");
    assert_eq!(Raw::code(refused[0]), Some("capacity_exceeded"));
    assert_eq!(app.knobs.spawns.load(Ordering::Acquire), 1);
    let (winner, loser) = if won[0]["result"]["value"]["managed_session_id"] == a.as_str() {
        (a, b)
    } else {
        (b, a)
    };
    // Over the bound: every attempt is refused at once, none is queued, none changes a thing.
    app.until(&winner, |d| d.session.runtime.actual == ActualState::Ready);
    let head = app.head();
    let mut raw = app.raw();
    raw.hello();
    for _ in 0..20 {
        let started = Instant::now();
        let over = raw.send("runtime/start", start_params(&c));
        assert_eq!(Raw::code(&over), Some("capacity_exceeded"), "{over}");
        assert!(started.elapsed() < Duration::from_millis(250));
    }
    assert_eq!(app.head(), head, "refusals leave no audit");
    assert_eq!(app.knobs.spawns.load(Ordering::Acquire), 1);
    assert_eq!(
        app.gui
            .session(c.clone())
            .unwrap()
            .session
            .runtime
            .generation,
        0
    );
    // The stopping slot counts: while the winner is being stopped, no one else starts.
    let ready = app.until(&winner, |d| d.session.runtime.actual == ActualState::Ready);
    app.gui
        .runtime_stop(
            uuid(),
            ready.session.runtime.fence.clone(),
            ready.session.runtime.revision,
        )
        .unwrap();
    let stopping = app.until(&winner, |d| {
        d.session.runtime.actual == ActualState::Stopping
    });
    assert_eq!(stopping.session.runtime.actual, ActualState::Stopping);
    let started = Instant::now();
    let during = raw.send("runtime/start", start_params(&loser));
    assert_eq!(Raw::code(&during), Some("capacity_exceeded"), "{during}");
    assert!(started.elapsed() < Duration::from_millis(250));
    assert_eq!(app.knobs.spawns.load(Ordering::Acquire), 1);
    // Reaped, the slot is free again, once.
    app.until(&winner, |d| {
        d.session.runtime.actual == ActualState::Stopped
    });
    let after = raw.send("runtime/start", start_params(&loser));
    assert!(after["error"].is_null(), "{after}");
    assert_eq!(app.knobs.spawns.load(Ordering::Acquire), 2);
}

// ---------------------------------------------------------------------------------- F8-08

#[test]
fn f8_08_staging_collisions_and_failures_leave_the_other_runtimes_catalogue_untouched() {
    use std::os::unix::fs::MetadataExt;
    let app = App::start(Options {
        limit: 2,
        staging: true,
        ..Options::default()
    });
    let (a, b) = (app.session(), app.session());
    let mut one = app.raw();
    one.hello();
    let mut two = app.raw();
    two.hello();
    // Simultaneous starts stage a catalogue each, in directories of their own.
    let gate = std::sync::Barrier::new(2);
    let (pa, pb) = (start_params(&a), start_params(&b));
    std::thread::scope(|scope| {
        let x = scope.spawn(|| {
            gate.wait();
            one.send("runtime/start", pa)
        });
        let y = scope.spawn(|| {
            gate.wait();
            two.send("runtime/start", pb)
        });
        assert!(x.join().unwrap()["error"].is_null());
        assert!(y.join().unwrap()["error"].is_null());
    });
    let da = app.until(&a, |d| d.session.runtime.actual == ActualState::Ready);
    let db = app.until(&b, |d| d.session.runtime.actual == ActualState::Ready);
    let file = |d: &SessionDetail| {
        app.staging
            .join(d.session.runtime.instance.as_ref().unwrap().as_str())
            .join("catalogue.json")
    };
    let (fa, fb) = (file(&da), file(&db));
    let (bytes_a, bytes_b) = (std::fs::read(&fa).unwrap(), std::fs::read(&fb).unwrap());
    assert_ne!(bytes_a, bytes_b);
    let inode_b = std::fs::metadata(&fb).unwrap().ino();
    assert_eq!(std::fs::read_dir(&app.staging).unwrap().count(), 2);
    // A stops; its next start fails after staging. Its directory is gone, B's file is as it was,
    // and the failed attempt consumed its generation.
    app.gui
        .runtime_stop(
            uuid(),
            da.session.runtime.fence.clone(),
            da.session.runtime.revision,
        )
        .unwrap();
    let stopped = app.until(&a, |d| d.session.runtime.actual == ActualState::Stopped);
    assert!(!fa.exists());
    app.knobs.spawn_fails.store(true, Ordering::Release);
    let failing = raw_start(&mut one, &a, &stopped);
    assert_eq!(Raw::code(&failing), Some("provider_rejected"), "{failing}");
    assert_eq!(std::fs::read(&fb).unwrap(), bytes_b);
    assert_eq!(std::fs::metadata(&fb).unwrap().ino(), inode_b);
    assert_eq!(std::fs::read_dir(&app.staging).unwrap().count(), 1);
    let consumed = app.gui.session(a.clone()).unwrap();
    assert_eq!(consumed.session.runtime.generation, 2);
    // B's file replaced by a link to a victim, and B stopped: cleanup deletes nothing outside
    // what it created.
    let victim = app.dir.parent().unwrap().join("victim.txt");
    std::fs::write(&victim, b"VICTIM").unwrap();
    std::fs::remove_file(&fb).unwrap();
    std::os::unix::fs::symlink(&victim, &fb).unwrap();
    app.gui
        .runtime_stop(
            uuid(),
            db.session.runtime.fence.clone(),
            db.session.runtime.revision,
        )
        .unwrap();
    app.until(&b, |d| d.session.runtime.actual == ActualState::Stopped);
    assert_eq!(std::fs::read(&victim).unwrap(), b"VICTIM");
    // A hard link to the victim is refused the same way.
    let relaunched = app.relaunch(&a, AgentMode::Inference);
    let fl = file(&relaunched);
    std::fs::remove_file(&fl).unwrap();
    std::fs::hard_link(&victim, &fl).unwrap();
    app.gui
        .runtime_stop(
            uuid(),
            relaunched.session.runtime.fence.clone(),
            relaunched.session.runtime.revision,
        )
        .unwrap();
    app.until(&a, |d| d.session.runtime.actual == ActualState::Stopped);
    assert_eq!(std::fs::read(&victim).unwrap(), b"VICTIM");
    assert!(victim.exists());
}
fn raw_start(raw: &mut Raw, session: &ManagedSessionId, detail: &SessionDetail) -> Value {
    raw.send(
        "runtime/start",
        json!({"intent_id":uuid().as_str(),"adapter_id":"codex","runtime_kind":"codex",
            "managed_session_id":session.as_str(),
            "expected_revision":detail.session.runtime.revision,
            "expected_generation":detail.session.runtime.generation}),
    )
}

// ---------------------------------------------------------------------------------- F8-09

/// The slowest answer of `ops` cheap control requests, with a pause between them.
fn worst_latency(raw: &mut Raw, session: &ManagedSessionId, ops: usize) -> Duration {
    let mut worst = Duration::ZERO;
    for _ in 0..ops {
        let started = Instant::now();
        let answer = raw.send(
            "runtime/status",
            json!({"adapter_id":"codex","runtime_kind":"codex","managed_session_id":session.as_str()}),
        );
        assert!(answer["error"].is_null(), "{answer}");
        worst = worst.max(started.elapsed());
        std::thread::sleep(Duration::from_millis(15));
    }
    worst
}

#[test]
fn f8_09_a_flood_and_a_blocked_shutdown_cannot_delay_bs_control() {
    let app = App::start(Options {
        limit: 2,
        tools: true,
        ..Options::default()
    });
    app.knobs.grace_ms.store(700, Ordering::Release);
    // A floods its lane with tens of megabytes, and ignores EOF and TERM when it is stopped.
    app.knobs.queue.lock().unwrap().push_back(
        json!({"flood": 200_000, "flood_count": 200, "ignore_term": true, "ignore_eof": true}),
    );
    let a = app.up(AgentMode::LocalTools);
    let b = app.up(AgentMode::LocalTools);
    let (_, approval_b) = app.waiting(&b);
    let intent = uuid();
    settled(|| {
        app.gui.turn_start(
            intent.clone(),
            a.fence.clone(),
            a.detail.session.revision,
            "flood".into(),
        )
    });
    let mut raw = app.raw();
    raw.hello();
    let ceiling = Duration::from_millis(250); // control_dispatch_deadline_ms
    // B is asked all the way through: while A floods, while the authority ends A, and while
    // A's stop is stuck on a child that ignores EOF and TERM.
    let mut worst = Duration::ZERO;
    let mut asked = 0;
    loop {
        worst = worst.max(worst_latency(&mut raw, &b.id, 5));
        asked += 5;
        let state = app
            .gui
            .session(a.id.clone())
            .unwrap()
            .session
            .runtime
            .actual;
        if asked >= 40 && matches!(state, ActualState::Stopped | ActualState::Failed) {
            break;
        }
        assert!(asked < 400, "A never finished stopping");
    }
    // B's own approval is answered meanwhile, and reaches only B's child.
    let started = Instant::now();
    let answered = settled(|| app.decide(&b, &approval_b, ApprovalDecision::Accept));
    assert_eq!(answered.state, ApprovalState::Accepted);
    worst = worst.max(started.elapsed());
    assert!(worst < ceiling, "B waited {worst:?} for the authority");
    eventually(|| (app.sent(&b.id, 1, "approval") == 1).then_some(()));
    // A's flood ended A, and only A: its turn did not complete.
    let ended = app.gui.session(a.id.clone()).unwrap();
    assert!(ended.turns[0].turn.state.terminal());
    assert_ne!(
        ended.turns[0].turn.state,
        crate::contracts::control::TurnState::Completed
    );
    assert_eq!(
        app.gui
            .session(b.id.clone())
            .unwrap()
            .session
            .runtime
            .actual,
        ActualState::Ready
    );
}

// ---------------------------------------------------------------------------------- F8-10

#[test]
fn f8_10_a_runtime_that_exceeds_its_bounds_stops_alone_and_preserves_bs_approval() {
    const KEYS: [&str; 5] = [
        "turn_output_bytes",
        "turn_events",
        "turn_events_per_second",
        "turn_event_burst",
        "provider_frame_bytes",
    ];
    for (name, behavior) in [
        // More bytes in one turn than the turn may have.
        ("bytes", json!({"flood": 200_000, "flood_count": 100})),
        // One frame over the provider frame bound (262,144 bytes).
        ("frame", json!({"oversize": 300_000})),
    ] {
        let app = App::start(Options {
            limit: 2,
            tools: true,
            ..Options::default()
        });
        app.knobs.queue.lock().unwrap().push_back(behavior);
        let a = app.up(AgentMode::LocalTools);
        let b = app.up(AgentMode::LocalTools);
        let (_, approval_b) = app.waiting(&b);
        let before = app
            .gui
            .approval(approval_b.approval.id.clone(), None, None)
            .unwrap();
        let intent = uuid();
        settled(|| {
            app.gui.turn_start(
                intent.clone(),
                a.fence.clone(),
                a.detail.session.revision,
                "go".into(),
            )
        });
        let failed = app.until(&a.id, |d| d.session.runtime.actual == ActualState::Failed);
        // What ended it is the authority's own record: a closed budget key when a budget did,
        // else the closed word for the lane or the protocol that gave way first.
        let rt = &failed.session.runtime;
        assert!(
            rt.budget_failure
                .as_deref()
                .is_none_or(|key| KEYS.contains(&key)),
            "{name}: {:?}",
            rt.budget_failure
        );
        assert!(
            rt.diagnostic.is_some() || rt.budget_failure.is_some(),
            "{name}"
        );
        assert!(
            failed
                .turns
                .first()
                .is_some_and(|t| t.turn.state.terminal()),
            "{name}"
        );
        // B: same identity, same deadline, same revision, and still decidable.
        let after = app
            .gui
            .approval(approval_b.approval.id.clone(), None, None)
            .unwrap();
        assert_eq!(
            after.approval.expires_at, before.approval.expires_at,
            "{name}"
        );
        assert_eq!(after.approval.revision, before.approval.revision, "{name}");
        assert!(after.approval.decidable, "{name}");
        assert_eq!(
            app.gui
                .session(b.id.clone())
                .unwrap()
                .session
                .runtime
                .actual,
            ActualState::Ready,
            "{name}"
        );
        let accepted = settled(|| app.decide(&b, &approval_b, ApprovalDecision::Accept));
        assert_eq!(accepted.state, ApprovalState::Accepted, "{name}");
        eventually(|| (app.sent(&b.id, 1, "approval") == 1).then_some(()));
        assert_eq!(app.sent(&a.id, 1, "approval"), 0, "{name}");
    }
}

// ---------------------------------------------------------------------------------- F8-11

#[test]
fn f8_11_one_turn_and_one_approval_per_session_but_distinct_sessions_coexist() {
    let app = App::start(Options {
        limit: 2,
        tools: true,
        ..Options::default()
    });
    let (a, b) = (app.up(AgentMode::LocalTools), app.up(AgentMode::LocalTools));
    let (turn_a, approval_a) = app.waiting(&a);
    // A second turn for A, while the first runs, is refused: by the core and by the index.
    let second = app.gui.turn_start(
        uuid(),
        a.fence.clone(),
        a.detail.session.revision,
        "again".into(),
    );
    let code = second.unwrap_err().code;
    assert!(
        matches!(code, F8Failure::TurnActive | F8Failure::PermissionDenied),
        "{code:?}"
    );
    assert_eq!(app.sent(&a.id, 1, "turn/start"), 1);
    // B has its own turn and its own approval at the same time as A.
    let (turn_b, approval_b) = app.waiting(&b);
    assert_ne!(turn_a, turn_b);
    for approval in [&approval_a, &approval_b] {
        let now = app
            .gui
            .approval(approval.approval.id.clone(), None, None)
            .unwrap();
        assert!(now.approval.decidable);
        assert_eq!(now.approval.state, ApprovalState::Pending);
    }
    assert_eq!(app.events("approval_requested"), 2);
}

#[test]
fn f8_11_a_second_action_proposed_while_one_is_pending_is_never_a_second_pending_approval() {
    let app = App::start(Options {
        limit: 2,
        tools: true,
        ..Options::default()
    });
    let a = app.up(AgentMode::LocalTools);
    let intent = uuid();
    let turn = settled(|| {
        app.gui.turn_start(
            intent.clone(),
            a.fence.clone(),
            a.detail.session.revision,
            "approve2".into(),
        )
    })
    .id;
    // The second proposal is a violation: the runtime is ended, the turn is indeterminate for a
    // runtime that was lost (not an authority that restarted), exactly one approval was ever
    // requested, and none is left to decide.
    let ended = app.until(&a.id, |d| d.session.runtime.actual == ActualState::Failed);
    assert_eq!(
        ended.session.runtime.diagnostic.map(|d| format!("{d:?}")),
        Some("TransportFailed".into())
    );
    let reading = app.gui.turn(turn.clone(), None, false).unwrap();
    assert_eq!(
        reading.turn.turn.state,
        crate::contracts::control::TurnState::Indeterminate
    );
    assert_eq!(
        reading.turn.turn.reason,
        Some(crate::contracts::control::TurnReason::RuntimeFailed)
    );
    assert_eq!(app.events("approval_requested"), 1);
    let requested = app.gui.audit_latest().unwrap();
    let first = requested
        .page
        .events
        .iter()
        .find_map(|e| e.approval.clone())
        .expect("the one approval that was requested is on record");
    let approval = app.gui.approval(first, None, None).unwrap();
    assert!(!approval.approval.decidable);
    assert_eq!(
        approval.approval.reason.map(|r| format!("{r:?}")),
        Some("RuntimeLost".into())
    );
}

// ---------------------------------------------------------------------------------- F8-13

#[test]
fn f8_13_a_crash_at_every_launch_stage_never_reuses_a_generation_or_fabricates_ready() {
    for stage in [
        LaunchStage::AfterReservation,
        LaunchStage::AfterSpawn,
        LaunchStage::AfterHandshake,
    ] {
        let app = App::start(Options {
            limit: 1,
            stage_fault: Some(stage),
            ..Options::default()
        });
        let a = app.session();
        let row = app.gui.session(a.clone()).unwrap().session;
        let started = app.gui.runtime_start(
            uuid(),
            a.clone(),
            row.runtime.revision,
            row.runtime.generation,
            AgentMode::Inference,
            None,
        );
        match stage {
            // The reservation and the spawn failed under the caller; the handshake fails later.
            LaunchStage::AfterReservation | LaunchStage::AfterSpawn => {
                assert!(started.is_err(), "{stage:?}")
            }
            LaunchStage::AfterHandshake => assert!(started.is_ok(), "{stage:?}"),
        }
        // It ends without ever having been ready, on its own, and its generation is spent.
        let ended = app.until(&a, |d| {
            matches!(
                d.session.runtime.actual,
                ActualState::Stopped | ActualState::Failed
            ) && d.session.runtime.generation == 1
        });
        assert_ne!(
            ended.session.runtime.actual,
            ActualState::Ready,
            "{stage:?}"
        );
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(
            app.gui.session(a.clone()).unwrap().session.runtime.actual != ActualState::Ready,
            true,
            "{stage:?}"
        );
        assert_eq!(app.events("runtime_ready"), 0, "{stage:?}");
        // No implicit restart; and an owned child that was spawned is stopped, not abandoned.
        let spawned = match stage {
            LaunchStage::AfterReservation => 0,
            _ => 1,
        };
        assert_eq!(
            app.knobs.spawns.load(Ordering::Acquire),
            spawned,
            "{stage:?}"
        );
        if let Some(pid) = app.pid_file(&a, 1, "pid") {
            eventually(|| (!alive(pid)).then_some(()));
        }
        // The next explicit start consumes the next generation with a brand-new identity.
        let again = app.relaunch(&a, AgentMode::Inference);
        assert_eq!(again.session.runtime.generation, 2, "{stage:?}");
        assert_ne!(
            again.session.runtime.instance,
            ended.session.runtime.instance
        );
        assert_eq!(app.events("runtime_ready"), 1, "{stage:?}");
    }
}

// ---------------------------------------------------------------------------------- F8-14

#[test]
fn f8_14_generation_overflow_refuses_without_spawning_and_replay_is_unchanged() {
    let app = App::start(Options {
        limit: 1,
        seed: Some(Box::new(|c| {
            // The counter of the newest seeded session is at the end of the range. The trigger
            // that guards the counter is put back as it was.
            let session: String = c
                .query_row(
                    "SELECT session_id FROM managed_sessions ORDER BY created_sequence DESC LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            let trigger: String = c
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE type='trigger' AND name='managed_runtimes_transition'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            c.execute_batch("DROP TRIGGER managed_runtimes_transition")
                .unwrap();
            c.execute(
                "INSERT INTO managed_runtimes(managed_session_id,adapter_id,runtime_kind,desired_state,revision,generation,created_at,updated_at) VALUES(?1,'codex','codex','stopped',1,9223372036854775807,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
                [&session],
            )
            .unwrap();
            c.execute_batch(&trigger).unwrap();
        })),
        ..Options::default()
    });
    let mut raw = app.raw();
    raw.hello();
    // An ordinary session starts, and its receipt is kept to be replayed.
    let fresh = app.session();
    let first = start_params(&fresh);
    let receipt = raw.send("runtime/start", first.clone());
    assert!(receipt["error"].is_null(), "{receipt}");
    let ready = app.until(&fresh, |d| d.session.runtime.actual == ActualState::Ready);
    app.gui
        .runtime_stop(
            uuid(),
            ready.session.runtime.fence.clone(),
            ready.session.runtime.revision,
        )
        .unwrap();
    app.until(&fresh, |d| d.session.runtime.actual == ActualState::Stopped);
    // The seeded session is the newest of the list, and the window is told its counter is at
    // its end instead of being handed a number it cannot hold.
    let listed = app.gui.sessions(None).unwrap();
    let exhausted = listed
        .sessions
        .iter()
        .find(|s| s.runtime.exhausted)
        .expect("a session whose counter is at its end");
    assert_eq!(exhausted.runtime.generation, 9_007_199_254_740_991);
    let (spawns, head) = (app.knobs.spawns.load(Ordering::Acquire), app.head());
    let overflow = raw.send(
        "runtime/start",
        json!({"intent_id":uuid().as_str(),"adapter_id":"codex","runtime_kind":"codex",
            "managed_session_id":exhausted.id.as_str(),"expected_revision":1,
            "expected_generation":9223372036854775807u64}),
    );
    assert_eq!(
        Raw::code(&overflow),
        Some("capacity_exceeded"),
        "{overflow}"
    );
    assert_eq!(
        app.knobs.spawns.load(Ordering::Acquire),
        spawns,
        "refused without a spawn"
    );
    assert_eq!(app.head(), head, "refused without a trace");
    // The earlier receipt replays unchanged, with nothing done again.
    let again = raw.send("runtime/start", first);
    assert_eq!(again["result"], receipt["result"], "{again}");
    assert_eq!(app.knobs.spawns.load(Ordering::Acquire), spawns);
    assert_eq!(app.head(), head);
}

// ---------------------------------------------------------------------------------- F8-15

#[test]
fn f8_15_a_exiting_with_a_stuck_stop_never_blocks_b_or_the_writer() {
    let app = App::start(Options {
        limit: 2,
        tools: true,
        ..Options::default()
    });
    app.knobs.grace_ms.store(600, Ordering::Release);
    app.knobs
        .queue
        .lock()
        .unwrap()
        .push_back(json!({"direct": true, "ignore_term": true, "ignore_eof": true}));
    let a = app.up(AgentMode::Inference);
    let b = app.up(AgentMode::LocalTools);
    let (turn_b, approval_b) = app.waiting(&b);
    let a_now = app.gui.session(a.id.clone()).unwrap();
    app.gui
        .runtime_stop(
            uuid(),
            a_now.session.runtime.fence.clone(),
            a_now.session.runtime.revision,
        )
        .unwrap();
    let mut raw = app.raw();
    raw.hello();
    // While A's stop escalates, B keeps its state, its turn and its approval, and the single
    // writer answers within the dispatch deadline.
    let mut worst = Duration::ZERO;
    let mut stuck = 0;
    loop {
        worst = worst.max(worst_latency(&mut raw, &b.id, 4));
        let state = app
            .gui
            .session(a.id.clone())
            .unwrap()
            .session
            .runtime
            .actual;
        if state == ActualState::Stopping {
            stuck += 1;
        }
        if state == ActualState::Stopped {
            break;
        }
        assert!(
            app.gui
                .session(b.id.clone())
                .unwrap()
                .session
                .runtime
                .actual
                == ActualState::Ready,
            "B keeps running while A stops"
        );
    }
    assert!(
        stuck >= 3,
        "A's stop really was stuck for a while ({stuck})"
    );
    assert!(worst < Duration::from_millis(250), "B waited {worst:?}");
    let still = app
        .gui
        .approval(approval_b.approval.id.clone(), None, None)
        .unwrap();
    assert!(still.approval.decidable);
    let accepted = settled(|| app.decide(&b, &approval_b, ApprovalDecision::Accept));
    assert_eq!(accepted.state, ApprovalState::Accepted);
    assert!(
        !app.gui
            .turn(turn_b, None, false)
            .unwrap()
            .turn
            .turn
            .state
            .terminal()
    );
}

// ---------------------------------------------------------------------------------- F8-16

#[test]
fn f8_16_audit_storage_full_fails_closed_at_admission_readiness_and_approval() {
    let app = App::start(Options {
        limit: 1,
        ..Options::default()
    });
    // At admission: nothing is reserved, spawned or held.
    let a = app.session();
    let row = app.gui.session(a.clone()).unwrap().session;
    app.knobs.audit_full.store(true, Ordering::Release);
    let head = app.head();
    let refused = app.gui.runtime_start(
        uuid(),
        a.clone(),
        row.runtime.revision,
        row.runtime.generation,
        AgentMode::Inference,
        None,
    );
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(app.knobs.spawns.load(Ordering::Acquire), 0);
    assert_eq!(app.head(), head);
    assert_eq!(
        app.gui
            .session(a.clone())
            .unwrap()
            .session
            .runtime
            .generation,
        0
    );
    app.knobs.audit_full.store(false, Ordering::Release);

    // At readiness: the start is accepted, then the disk fills before the runtime is attested.
    app.gui
        .runtime_start(
            uuid(),
            a.clone(),
            row.runtime.revision,
            row.runtime.generation,
            AgentMode::Inference,
            None,
        )
        .unwrap();
    app.knobs.audit_full.store(true, Ordering::Release);
    std::thread::sleep(Duration::from_millis(800));
    let pid = app.pid_file(&a, 1, "pid");
    let held = app.gui.session(a.clone()).unwrap();
    // Never reported ready; its cleanup still completes (the child is gone), the exit that
    // could not be recorded is retried, not lost.
    assert_ne!(held.session.runtime.actual, ActualState::Ready);
    if let Some(pid) = pid {
        eventually(|| (!alive(pid)).then_some(()));
    }
    app.knobs.audit_full.store(false, Ordering::Release);
    let after = app.until(&a, |d| {
        matches!(
            d.session.runtime.actual,
            ActualState::Stopped | ActualState::Failed
        ) && app.events("runtime_exited") == 1
    });
    assert_eq!(app.events("runtime_ready"), 0);
    assert!(after.session.runtime.generation >= 1);
}

#[test]
fn f8_16_a_decision_that_cannot_be_recorded_is_never_written_to_the_provider() {
    let app = App::start(Options {
        limit: 1,
        tools: true,
        ..Options::default()
    });
    app.knobs
        .queue
        .lock()
        .unwrap()
        .push_back(json!({"approval_delay_ms": 600}));
    let s = app.up(AgentMode::LocalTools);
    let (_, approval) = app.waiting(&s);
    // The audit table refuses: the decision is not recorded, so it is not sent.
    app.knobs.audit_full.store(true, Ordering::Release);
    let refused = app.decide(&s, &approval, ApprovalDecision::Accept);
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(app.sent(&s.id, 1, "approval"), 0);
    app.knobs.audit_full.store(false, Ordering::Release);
    let still = app
        .gui
        .approval(approval.approval.id.clone(), None, None)
        .unwrap();
    assert_eq!(still.approval.state, ApprovalState::Pending);
    assert!(still.approval.decidable);
    // Recorded and sent, then the audit fills before the provider's confirmation arrives: the
    // outcome is not invented, and nothing is sent again.
    let decision = uuid();
    settled(|| {
        app.gui.approval_respond(
            decision.clone(),
            s.fence.clone(),
            approval.approval.id.clone(),
            approval.approval.revision,
            ApprovalDecision::Accept,
        )
    });
    app.knobs.audit_full.store(true, Ordering::Release);
    std::thread::sleep(Duration::from_millis(1200));
    let mut raw = app.raw();
    raw.hello();
    let unknown = raw.send(
        "managedTurn/approval/status",
        json!({"managed_approval_id": approval.approval.id.as_str()}),
    );
    assert_ne!(
        unknown["result"]["value"]["approval"]["effect"], "confirmed",
        "a confirmation that could not be recorded is not shown as recorded: {unknown}"
    );
    assert_ne!(
        unknown["result"]["value"]["delivery"], "effect_confirmed",
        "{unknown}"
    );
    assert_eq!(app.sent(&s.id, 1, "approval"), 1);
    app.knobs.audit_full.store(false, Ordering::Release);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(app.sent(&s.id, 1, "approval"), 1, "never sent again");
}

// ---------------------------------------------------------------------------------- F8-17

#[test]
fn f8_17_only_the_owned_child_is_signalled_and_descendants_are_reported_not_killed() {
    let app = App::start(Options {
        limit: 1,
        ..Options::default()
    });
    app.knobs.grace_ms.store(100, Ordering::Release);
    // A process unrelated to the authority, running the very same executable as the child.
    let mut bystander = Command::new("/usr/bin/python3")
        .args(["-c", "import time; time.sleep(60)"])
        .spawn()
        .unwrap();
    // The child ignores EOF and TERM, and starts a descendant of its own. Both announce their
    // PID to files this test named: no process is ever found by name, by the code under test
    // or by the test.
    app.knobs.queue.lock().unwrap().push_back(json!({
        "direct": true, "ignore_term": true, "ignore_eof": true, "grandchild": true}));
    let a = app.session();
    let ready = app.ready(&a, AgentMode::Inference);
    let child = eventually(|| app.pid_file(&a, 1, "pid"));
    let grandchild = eventually(|| app.pid_file(&a, 1, "grandchild"));
    assert!(alive(child) && alive(grandchild));
    app.gui
        .runtime_stop(
            uuid(),
            ready.session.runtime.fence.clone(),
            ready.session.runtime.revision,
        )
        .unwrap();
    app.until(&a, |d| d.session.runtime.actual == ActualState::Stopped);
    // The owned child is gone, signalled through its verified lifecycle even though it ignored
    // EOF and TERM. The unrelated process is untouched. The descendant, which nobody owns, is
    // reported as left behind and is not hunted down.
    assert!(!alive(child));
    assert!(bystander.try_wait().unwrap().is_none());
    assert!(
        alive(grandchild),
        "a descendant the authority does not own is not killed"
    );
    // This test cleans up exactly the two processes whose PIDs it was told.
    let pinned = |pid: u32| unsafe { nix::libc::syscall(nix::libc::SYS_pidfd_open, pid as i32, 0) };
    let fd = pinned(grandchild);
    assert!(fd >= 0);
    // SAFETY: signals only the descendant this test pinned through its pidfd, then closes it.
    unsafe {
        nix::libc::syscall(
            nix::libc::SYS_pidfd_send_signal,
            fd as i32,
            nix::libc::SIGKILL,
            std::ptr::null::<nix::libc::siginfo_t>(),
            0,
        );
        nix::libc::close(fd as i32);
    }
    bystander.kill().unwrap();
    bystander.wait().unwrap();
}

#[test]
fn f8_17_a_profile_that_runs_tools_is_refused_where_descendants_are_not_contained() {
    // Containment is claimed by the spawner of an application started for tools. This one
    // claims none: Local Tools is refused before anything is reserved, spawned or recorded.
    let app = App::start(Options {
        limit: 1,
        tools: false,
        ..Options::default()
    });
    let a = app.session();
    let row = app.gui.session(a.clone()).unwrap().session;
    let head = app.head();
    let refused = app.gui.runtime_start(
        uuid(),
        a.clone(),
        row.runtime.revision,
        row.runtime.generation,
        AgentMode::LocalTools,
        Some(app.workspace()),
    );
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(app.knobs.spawns.load(Ordering::Acquire), 0);
    assert_eq!(app.head(), head);
    assert_eq!(app.gui.session(a).unwrap().session.runtime.generation, 0);
}

// ---------------------------------------------------------------------------------- F8-18

#[test]
fn f8_18_a_workspace_or_profile_replaced_during_dispatch_writes_nothing() {
    let app = App::start(Options {
        limit: 1,
        tools: true,
        ..Options::default()
    });
    let s = app.up(AgentMode::LocalTools);
    let (turn, approval) = app.waiting(&s);
    // The profile is replaced: the runtime that ran Local Tools is stopped, and an Inference
    // runtime takes its place. The binding was made for the other profile.
    app.wind_down(&s, &turn);
    let inference = app.relaunch(&s.id, AgentMode::Inference);
    assert_eq!(inference.session.runtime.generation, 2);
    // The session is attached again, and now in the profile of the runtime it is attached to:
    // the profile of the earlier attachment is not carried over.
    let resume_intent = uuid();
    settled(|| {
        app.gui.binding_resume(
            resume_intent.clone(),
            inference.session.runtime.fence.clone(),
            inference.session.revision,
        )
    });
    let attached = app.until(&s.id, |d| d.binding.state == BindingState::BoundAttached);
    assert_eq!(
        attached.binding.mode,
        Some(crate::contracts::control::AttachmentProfile::InferenceOnlyV1)
    );
    // What Local Tools allowed is not allowed here: no command and no file change, though the
    // lease still carries the scopes.
    for gate in &attached.gates {
        if gate.action != ExecutionScope::TurnStart {
            assert_eq!(
                gate.reason,
                crate::contracts::control::GateReason::LocalToolsProfileMissing,
                "{gate:?}"
            );
        }
    }
    // The old ticket and the old approval are dead: nothing is written for them to the new child.
    let old = app.decide(&s, &approval, ApprovalDecision::Accept);
    assert!(old.is_err(), "{old:?}");
    assert_eq!(app.sent(&s.id, 1, "approval"), 0);
    assert_eq!(app.sent(&s.id, 2, "approval"), 0);
    assert_eq!(app.sent(&s.id, 2, "turn/start"), 0);
    // Every frame the new child got is the attachment it was given, and nothing else.
    for line in app.frames(&s.id, 2) {
        let frame: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(frame["method"], "thread/resume", "{line}");
    }
    // The workspace is replaced: the runtime is asked to be isolated for one folder and reports
    // another. It is never ready, and is stopped as unproven.
    let other = app.dir.parent().unwrap().join("other-workspace");
    std::fs::create_dir(&other).unwrap();
    let now = app.gui.session(s.id.clone()).unwrap();
    app.gui
        .runtime_stop(
            uuid(),
            now.session.runtime.fence.clone(),
            now.session.runtime.revision,
        )
        .unwrap();
    app.until(&s.id, |d| d.session.runtime.actual == ActualState::Stopped);
    let row = app.gui.session(s.id.clone()).unwrap().session;
    app.gui
        .runtime_start(
            uuid(),
            s.id.clone(),
            row.runtime.revision,
            row.runtime.generation,
            AgentMode::LocalTools,
            Some(other.to_str().unwrap().to_owned()),
        )
        .unwrap();
    let rejected = app.until(&s.id, |d| {
        d.session.runtime.generation == 3
            && matches!(
                d.session.runtime.actual,
                ActualState::Failed | ActualState::Stopped
            )
    });
    assert_eq!(
        rejected
            .session
            .runtime
            .diagnostic
            .map(|d| format!("{d:?}")),
        Some("SafetyUnproven".into())
    );
    assert!(app.frames(&s.id, 3).is_empty());
    assert_eq!(app.sent(&s.id, 1, "approval"), 0);
}

// ------------------------------------------------------------------------ the 29 methods

/// The call is answered with a result, asking again while the authority is still working on
/// the very same request.
fn ok(raw: &mut Raw, method: &str, params: Value) -> Value {
    eventually(|| {
        let answer = raw.send(method, params.clone());
        match Raw::code(&answer) {
            Some("runtime_operation_in_progress") => None,
            Some(code) => panic!("{method} was refused as {code}: {answer}"),
            None => Some(answer),
        }
    })
}
fn value(answer: &Value) -> &Value {
    &answer["result"]["value"]
}

#[test]
fn all_29_methods_are_answered_and_refused_through_the_transport() {
    let app = App::start(Options {
        limit: 2,
        tools: true,
        ..Options::default()
    });
    let mut raw = app.raw();
    let log = raw.log.clone();
    let hello = raw.hello();
    assert_eq!(value(&hello)["protocol_version"], 2);
    let status = ok(&mut raw, "authority/status", json!({}));
    assert_eq!(value(&status)["storage_schema_version"], 8);

    // Sessions.
    let prepared = ok(
        &mut raw,
        "managedSession/prepare",
        json!({"intent_id":uuid().as_str(),"adapter_id":"codex","runtime_kind":"codex"}),
    );
    let session = value(&prepared)["managed_session_id"]
        .as_str()
        .unwrap()
        .to_owned();
    ok(
        &mut raw,
        "managedSession/get",
        json!({"managed_session_id":session}),
    );
    let listed = ok(
        &mut raw,
        "managedSession/list",
        json!({"after":null,"limit":50}),
    );
    assert!(value(&listed)["sessions"].as_array().unwrap().len() >= 2);

    // Runtime.
    let workspace = app.workspace();
    let target = json!({"adapter_id":"codex","runtime_kind":"codex","managed_session_id":session});
    ok(
        &mut raw,
        "runtime/start",
        json!({"intent_id":uuid().as_str(),"adapter_id":"codex","runtime_kind":"codex",
        "managed_session_id":session,"expected_revision":0,"expected_generation":0,
        "safety_profile":"codex_local_tools_v1","workspace_root":workspace}),
    );
    let live = eventually(|| {
        let status = ok(&mut raw, "runtime/status", target.clone());
        (value(&status)["actual_state"] == "ready").then_some(status)
    });
    let identity = &value(&live)["runtime_identity"];
    let fence = json!({"managed_session_id":session,"generation":identity["generation"],
        "runtime_instance_id":identity["runtime_instance_id"]});
    // The fixture child lists no models; the listing answers that it is not there to read.
    let models = raw.send(
        "runtime/model/list",
        json!({"adapter_id":"codex","runtime_kind":"codex",
        "managed_session_id":session,"expected_generation":identity["generation"],
        "expected_runtime_instance_id":identity["runtime_instance_id"],"cursor":null,"limit":20}),
    );
    assert_eq!(
        Raw::code(&models),
        Some("runtime_operation_in_progress"),
        "{models}"
    );

    // Leases and the gate.
    let lease = ok(
        &mut raw,
        "executionLease/grant",
        json!({"intent_id":uuid().as_str(),"session_id":session,
        "expected_session_revision":1,"scopes":["session_create","session_resume","turn_start","command_execute","file_change"],
        "ttl_seconds":300,"runtime_fence":fence}),
    );
    let lease_id = value(&lease)["lease_id"].as_str().unwrap().to_owned();
    ok(&mut raw, "executionLease/get", json!({"lease_id":lease_id}));
    let active = ok(
        &mut raw,
        "executionLease/active",
        json!({"session_id":session}),
    );
    assert_eq!(value(&active)["lease_id"], lease_id.as_str());
    ok(
        &mut raw,
        "executionLease/renew",
        json!({"intent_id":uuid().as_str(),"lease_id":lease_id,
        "expected_revision":1,"ttl_seconds":600,"runtime_fence":fence}),
    );
    ok(
        &mut raw,
        "executionGate/status",
        json!({"session_id":session,"action":"turn_start"}),
    );

    // Binding.
    ok(
        &mut raw,
        "managedSession/runtime/create",
        json!({"intent_id":uuid().as_str(),"managed_session_id":session,
        "expected_session_revision":1,"workspace_root":workspace,"runtime_fence":fence}),
    );
    let bound = eventually(|| {
        let r = ok(
            &mut raw,
            "managedSession/runtime/status",
            json!({"managed_session_id":session}),
        );
        (value(&r)["state"] == "bound_attached").then_some(r)
    });
    assert_eq!(value(&bound)["attachment_profile"], "local_tools_v1");

    // A turn, its stream, its approval and its end.
    let turn = ok(
        &mut raw,
        "managedSession/turn/start",
        json!({"intent_id":uuid().as_str(),
        "managed_session_id":session,"expected_session_revision":1,"prompt":"approve","runtime_fence":fence}),
    );
    let turn_id = value(&turn)["managed_turn_id"].as_str().unwrap().to_owned();
    ok(
        &mut raw,
        "managedSession/turn/status",
        json!({"managed_turn_id":turn_id}),
    );
    let pending = eventually(|| {
        let r = ok(
            &mut raw,
            "managedTurn/approval/pending",
            json!({"managed_turn_id":turn_id}),
        );
        value(&r).is_object().then_some(r)
    });
    let approval_id = value(&pending)["approval"]["managed_approval_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let events = eventually(|| {
        let r = ok(
            &mut raw,
            "managedSession/turn/events",
            json!({"managed_turn_id":turn_id,"after_sequence":null,"limit":50}),
        );
        (!value(&r)["events"].as_array().unwrap().is_empty()).then_some(r)
    });
    assert!(events.to_string().contains("fixture stream"));
    let shown = ok(
        &mut raw,
        "managedTurn/approval/status",
        json!({"managed_approval_id":approval_id}),
    );
    assert_eq!(value(&shown)["acceptable"], true);
    ok(
        &mut raw,
        "managedTurn/approval/respond",
        json!({"intent_id":uuid().as_str(),"managed_approval_id":approval_id,
        "expected_revision":1,"decision":"decline","runtime_fence":fence}),
    );
    ok(
        &mut raw,
        "managedSession/turn/cancel",
        json!({"intent_id":uuid().as_str(),"managed_turn_id":turn_id,
        "expected_revision":2,"runtime_fence":fence}),
    );
    eventually(|| {
        let r = ok(
            &mut raw,
            "managedSession/turn/status",
            json!({"managed_turn_id":turn_id}),
        );
        (value(&r)["turn"]["state"] == "interrupted").then_some(())
    });

    // The durable record.
    let stream = value(&status)["stream_id"].as_str().unwrap().to_owned();
    let read = ok(
        &mut raw,
        "audit/read",
        json!({"stream_id":stream,"after_sequence":null,"limit":50}),
    );
    assert!(!value(&read)["events"].as_array().unwrap().is_empty());
    let head = value(&ok(&mut raw, "authority/status", json!({})))["last_sequence"].clone();
    ok(
        &mut raw,
        "audit/watch",
        json!({"stream_id":stream,"after_sequence":null,"limit":5,"wait_ms":0}),
    );
    let _ = head;

    // A new launch of the runtime, and the binding resumed on it under the lease that is held.
    let status = ok(&mut raw, "runtime/status", target.clone());
    ok(
        &mut raw,
        "runtime/stop",
        json!({"intent_id":uuid().as_str(),"adapter_id":"codex","runtime_kind":"codex",
            "managed_session_id":session,"expected_revision":value(&status)["revision"],
            "expected_generation":identity["generation"],
            "expected_runtime_instance_id":identity["runtime_instance_id"]}),
    );
    let stopped = eventually(|| {
        let r = ok(&mut raw, "runtime/status", target.clone());
        (value(&r)["actual_state"] == "stopped").then_some(r)
    });
    ok(
        &mut raw,
        "runtime/start",
        json!({"intent_id":uuid().as_str(),"adapter_id":"codex","runtime_kind":"codex",
            "managed_session_id":session,"expected_revision":value(&stopped)["revision"],
            "expected_generation":value(&stopped)["generation"],
            "safety_profile":"codex_local_tools_v1","workspace_root":workspace}),
    );
    let again = eventually(|| {
        let r = ok(&mut raw, "runtime/status", target.clone());
        (value(&r)["actual_state"] == "ready").then_some(r)
    });
    let identity = &value(&again)["runtime_identity"];
    assert_eq!(identity["generation"], 2);
    let fence = json!({"managed_session_id":session,"generation":identity["generation"],
        "runtime_instance_id":identity["runtime_instance_id"]});
    ok(
        &mut raw,
        "managedSession/runtime/resume",
        json!({"intent_id":uuid().as_str(),"managed_session_id":session,
            "expected_session_revision":1,"runtime_fence":fence}),
    );
    eventually(|| {
        let r = ok(
            &mut raw,
            "managedSession/runtime/status",
            json!({"managed_session_id":session}),
        );
        (value(&r)["state"] == "bound_attached").then_some(())
    });

    // Ending: revoke, stop, close.
    let now = ok(&mut raw, "executionLease/get", json!({"lease_id":lease_id}));
    ok(
        &mut raw,
        "executionLease/revoke",
        json!({"intent_id":uuid().as_str(),"lease_id":lease_id,
            "expected_revision":value(&now)["revision"]}),
    );
    let status = ok(&mut raw, "runtime/status", target.clone());
    ok(
        &mut raw,
        "runtime/stop",
        json!({"intent_id":uuid().as_str(),"adapter_id":"codex","runtime_kind":"codex",
            "managed_session_id":session,"expected_revision":value(&status)["revision"],
            "expected_generation":identity["generation"],
            "expected_runtime_instance_id":identity["runtime_instance_id"]}),
    );
    let stopped = eventually(|| {
        let r = ok(&mut raw, "runtime/status", target.clone());
        (value(&r)["actual_state"] == "stopped").then_some(r)
    });
    ok(
        &mut raw,
        "managedSession/close",
        json!({"intent_id":uuid().as_str(),"managed_session_id":session,
            "expected_revision":1,"runtime_fence":{"managed_session_id":session,
            "generation":value(&stopped)["generation"],"runtime_instance_id":null}}),
    );

    // A binding whose creation the runtime did not survive is uncertain, and is abandoned.
    app.knobs
        .queue
        .lock()
        .unwrap()
        .push_back(json!({"crash_on_thread": true}));
    let other = ok(
        &mut raw,
        "managedSession/prepare",
        json!({"intent_id":uuid().as_str(),"adapter_id":"codex","runtime_kind":"codex"}),
    );
    let other = value(&other)["managed_session_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let other_target =
        json!({"adapter_id":"codex","runtime_kind":"codex","managed_session_id":other});
    ok(
        &mut raw,
        "runtime/start",
        json!({"intent_id":uuid().as_str(),"adapter_id":"codex","runtime_kind":"codex",
        "managed_session_id":other,"expected_revision":0,"expected_generation":0,
        "safety_profile":"codex_local_tools_v1","workspace_root":workspace}),
    );
    let live = eventually(|| {
        let r = ok(&mut raw, "runtime/status", other_target.clone());
        (value(&r)["actual_state"] == "ready").then_some(r)
    });
    let identity = &value(&live)["runtime_identity"];
    let fence = json!({"managed_session_id":other,"generation":identity["generation"],"runtime_instance_id":identity["runtime_instance_id"]});
    ok(
        &mut raw,
        "executionLease/grant",
        json!({"intent_id":uuid().as_str(),"session_id":other,"expected_session_revision":1,
        "scopes":["session_create"],"ttl_seconds":300,"runtime_fence":fence}),
    );
    let create = json!({"intent_id":uuid().as_str(),"managed_session_id":other,"expected_session_revision":1,
        "workspace_root":workspace,"runtime_fence":fence});
    raw.send("managedSession/runtime/create", create);
    let uncertain = eventually(|| {
        let r = ok(
            &mut raw,
            "managedSession/runtime/status",
            json!({"managed_session_id":other}),
        );
        (value(&r)["state"] == "indeterminate").then_some(r)
    });
    let operation = value(&uncertain)["operation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let dead = eventually(|| {
        let r = ok(&mut raw, "runtime/status", other_target.clone());
        matches!(
            value(&r)["actual_state"].as_str(),
            Some("failed" | "stopped")
        )
        .then_some(r)
    });
    ok(
        &mut raw,
        "managedSession/runtime/abandon",
        json!({"intent_id":uuid().as_str(),"managed_session_id":other,
        "operation_id":operation,"runtime_fence":{"managed_session_id":other,"generation":value(&dead)["generation"],"runtime_instance_id":null}}),
    );
    let after = ok(
        &mut raw,
        "managedSession/runtime/status",
        json!({"managed_session_id":other}),
    );
    assert_eq!(value(&after)["state"], "unbound");

    // Every method also refuses what is not its shape: unknown keys are never ignored.
    for method in crate::contracts::control::METHODS {
        if method == "authority/hello" {
            continue;
        }
        let refused = raw.send(method, json!({"unknown":1}));
        assert!(
            matches!(Raw::code(&refused), Some("invalid_request")),
            "{method}: {refused}"
        );
    }

    // The ledger: every one of the 29 was answered with a result and was refused as well.
    let log = log.lock().unwrap();
    for method in crate::contracts::control::METHODS {
        let answered = log.iter().any(|(m, ok)| m == method && *ok);
        let refused = log.iter().any(|(m, ok)| m == method && !*ok);
        // The listing of models can only be refused by the fixture; its answer is below.
        if method == "runtime/model/list" {
            assert!(refused, "{method} was never refused");
            continue;
        }
        assert!(answered, "{method} was never answered with a result");
        assert!(
            refused || method == "authority/hello",
            "{method} was never refused"
        );
    }
}

#[test]
fn the_model_listing_is_answered_for_the_launch_that_made_it_and_for_no_other() {
    use crate::f8::gui::harness::NativeHarness;
    let h = NativeHarness::start("approval_command");
    let mut raw = Raw::connect(&h.handle);
    raw.hello();
    let session = value(&ok(
        &mut raw,
        "managedSession/prepare",
        json!({"intent_id":uuid().as_str(),"adapter_id":"codex","runtime_kind":"codex"}),
    ))["managed_session_id"]
        .as_str()
        .unwrap()
        .to_owned();
    ok(
        &mut raw,
        "runtime/start",
        json!({"intent_id":uuid().as_str(),"adapter_id":"codex","runtime_kind":"codex",
        "managed_session_id":session,"expected_revision":0,"expected_generation":0,
        "safety_profile":"codex_local_tools_v1","workspace_root":h.workspace}),
    );
    let target = json!({"adapter_id":"codex","runtime_kind":"codex","managed_session_id":session});
    let live = eventually(|| {
        let r = ok(&mut raw, "runtime/status", target.clone());
        (value(&r)["actual_state"] == "ready").then_some(r)
    });
    let identity = value(&live)["runtime_identity"].clone();
    let page = |cursor: Value, instance: &Value| {
        json!({"adapter_id":"codex","runtime_kind":"codex","managed_session_id":session,
            "expected_generation":identity["generation"],"expected_runtime_instance_id":instance,
            "cursor":cursor,"limit":20})
    };
    let first = ok(
        &mut raw,
        "runtime/model/list",
        page(Value::Null, &identity["runtime_instance_id"]),
    );
    assert_eq!(value(&first)["models"].as_array().unwrap().len(), 20);
    assert_eq!(
        value(&first)["runtime_instance_id"],
        identity["runtime_instance_id"]
    );
    assert_eq!(value(&live)["model_count"], 45);
    let next = value(&first)["next"].clone();
    let second = ok(
        &mut raw,
        "runtime/model/list",
        page(next.clone(), &identity["runtime_instance_id"]),
    );
    assert_eq!(value(&second)["models"].as_array().unwrap().len(), 20);
    // A cursor made for one launch is refused for another instance, and so is a stale launch.
    let foreign = json!(uuid().as_str());
    let crossed = raw.send("runtime/model/list", page(next, &foreign));
    assert_refused(&crossed, &["invalid_request", "stale_revision"]);
    let stale = raw.send("runtime/model/list", page(Value::Null, &foreign));
    assert_refused(&stale, &["stale_revision"]);
}

#[test]
fn a_binding_created_by_a_runtime_that_did_not_survive_is_uncertain_and_is_abandoned_through_the_window()
 {
    let app = App::start(Options {
        limit: 1,
        tools: true,
        ..Options::default()
    });
    app.knobs
        .queue
        .lock()
        .unwrap()
        .push_back(json!({"crash_on_thread": true}));
    let id = app.session();
    let ready = app.ready(&id, AgentMode::LocalTools);
    let fence = ready.session.runtime.fence.clone();
    app.gui
        .lease_grant(
            uuid(),
            fence.clone(),
            ready.session.revision,
            ALL.to_vec(),
            300,
        )
        .unwrap();
    // The runtime dies as it is asked to create the thread. The answer is not a success.
    let created = app.gui.binding_create(
        uuid(),
        fence.clone(),
        ready.session.revision,
        app.workspace(),
    );
    assert!(created.is_err(), "{created:?}");
    // The binding is not left waiting for a provider that is gone: it is uncertain, and the
    // window is given the operation to discard.
    let uncertain = app.until(&id, |d| d.binding.state == BindingState::Indeterminate);
    let operation = uncertain
        .binding
        .operation
        .clone()
        .expect("the operation to abandon");
    assert_eq!(uncertain.session.runtime.actual, ActualState::Failed);
    app.gui
        .binding_abandon(uuid(), uncertain.session.runtime.fence.clone(), operation)
        .unwrap();
    let clean = app.until(&id, |d| d.binding.state == BindingState::Unbound);
    assert!(clean.binding.operation.is_none());
    assert_eq!(app.events("binding_indeterminate"), 1);
    assert_eq!(app.events("binding_abandoned"), 1);
}

#[path = "acceptance_system.rs"]
mod system;
