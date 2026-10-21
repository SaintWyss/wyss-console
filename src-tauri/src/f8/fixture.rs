//! A runtime for looking at the product without a provider: a child process that speaks the
//! explicitly normalized test protocol, inside bubblewrap. It is **not Codex** and it is not
//! in any shipped binary: it is compiled only with `f8-dev` (the window's development
//! application) or `f8-fixture` (the isolated candidate authority `wyss-f8-fixture`), and
//! `check:package-fixtures` refuses a package that carries it. It touches nothing outside the
//! workspace it is given.
use super::{
    engine::{Attachment, Containment, Spawned, Spawner, StopState, Worker},
    ownership::{ContainmentPolicy, Grace},
    process_worker::DeferredProcessWorker,
    protocol::RuntimeRef,
    staging::LaunchStaging,
};
use crate::contracts::control::ControlResult;
use serde_json::json;
use std::{
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

const SCRIPT: &str = r#"
import json,sys
workspace,profile,metadata=sys.argv[1:4]
metadata=json.loads(metadata)
def emit(v): print(json.dumps(v,separators=(',',':')),flush=True)
emit(dict(kind='runtime_ready',observed_version='0.159.0',workspace=workspace,profile=profile,revision=1))
active=None
for line in sys.stdin:
    if line.startswith('approval '):
        _,item,decision=line.strip().split(' ')
        emit(dict(kind='approval_item_consumed',provider_item_id=item))
        if decision=='accept': emit(dict(kind='approval_item_confirmed',provider_item_id=item))
        continue
    v=json.loads(line)
    if v['method'] in ('thread/start','thread/resume'):
        emit(dict(kind='binding_confirmed',operation_id=v['operation_id'],provider_thread_id=v.get('thread_id') or 'fixture-'+v['operation_id']))
    elif v['method']=='turn/start':
        active=v['turn']
        emit(dict(kind='turn_running',managed_turn_id=active,provider_turn_id='provider-'+active))
        emit(dict(kind='content',managed_turn_id=active,provider_turn_id='provider-'+active,text='This is a fixture runtime, not Codex. '))
        emit(dict(kind='content',managed_turn_id=active,provider_turn_id='provider-'+active,text='It answers, and asks to run one command.'))
        if v['input'].startswith('ask'):
            emit(dict(kind='approval_required',approval_kind='command_execution',provider_item_id='fixture-item',action_fingerprint=metadata['fingerprint'],structure=metadata['structure'],action=dict(kind='command',command='printf fixture',cwd=workspace,parsed_actions=[])))
        else:
            emit(dict(kind='turn_completed',managed_turn_id=active,provider_turn_id='provider-'+active))
            active=None
    elif v['method']=='turn/interrupt' and active:
        emit(dict(kind='turn_interrupted',managed_turn_id=active))
        active=None
"#;

/// The wrapper that lets the normalized fixture stand in for a proven provider preflight.
struct FixtureWorker(DeferredProcessWorker);
impl Worker for FixtureWorker {
    fn preflight(&mut self, _: &Attachment) -> ControlResult<()> {
        Ok(())
    }
    fn send(&mut self, frame: &[u8]) -> ControlResult<()> {
        self.0.send(frame)
    }
    fn send_critical(&mut self, frame: &[u8]) -> ControlResult<()> {
        self.0.send_critical(frame)
    }
    fn flush(&mut self) -> ControlResult<()> {
        self.0.flush()
    }
    fn inbound(&mut self, n: usize) -> ControlResult<Vec<Vec<u8>>> {
        self.0.inbound(n)
    }
    fn has_exited(&mut self) -> bool {
        self.0.has_exited()
    }
    fn containment(&mut self) -> Containment {
        self.0.containment()
    }
    fn begin_stop(&mut self, now: Instant) {
        self.0.begin_stop(now);
    }
    fn poll_stop(&mut self, now: Instant) -> StopState {
        self.0.poll_stop(now)
    }
}

pub struct FixtureSpawner {
    workspace: PathBuf,
}
impl Spawner for FixtureSpawner {
    fn descendant_containment(&self) -> bool {
        true
    }
    fn spawn(&mut self, _: &RuntimeRef, _: Option<&mut LaunchStaging>) -> ControlResult<Spawned> {
        let action = super::operations::ProviderAction::Command {
            command: "printf fixture".into(),
            cwd: self.workspace.to_str().unwrap_or("/").into(),
            parsed_actions: vec![],
        };
        let (_, fingerprint, structure) = action.metadata("fixture-item")?;
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
            .args(["-u", "-c", SCRIPT])
            .arg(&self.workspace)
            .arg("codex_local_tools_v1")
            .arg(json!({"fingerprint":fingerprint,"structure":structure}).to_string());
        Ok(Spawned {
            worker: Box::new(FixtureWorker(DeferredProcessWorker::spawn(
                command,
                Grace {
                    eof: Duration::from_millis(200),
                    term: Duration::from_millis(200),
                    kill: Duration::from_secs(1),
                },
                ContainmentPolicy::DescendantsContained,
            )?)),
        })
    }
}

impl FixtureSpawner {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}
