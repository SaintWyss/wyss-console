//! The actual control transport, durable authority and owned ProcessWorker together.
//! This fixture speaks the explicitly normalized test protocol; it is not Codex acceptance.
use super::{
    authority::Authority,
    engine::{Attachment, Containment, Engine, Spawned, Spawner, StopState, Worker},
    migration,
    ownership::{ContainmentPolicy, Grace},
    process_worker::ProcessWorker,
    protocol::RuntimeRef,
    registry::Registry,
    service::Service,
    staging::LaunchStaging,
};
use crate::{
    contracts::{
        WireUuid,
        control::{ControlResult, unique_json},
    },
    control::{F8Transport, f8_fixture},
};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

const AUTH: &str = "00000000-0000-4000-8000-0000000000a1";
const SCRIPT: &str = r#"
import json,sys
workspace=sys.argv[1]
profile=sys.argv[2]
metadata=json.loads(sys.argv[3])
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
        emit(dict(kind='binding_confirmed',operation_id=v['operation_id'],provider_thread_id='fixture-'+v['operation_id']))
    elif v['method']=='turn/start':
        active=v['turn']
        emit(dict(kind='turn_running',managed_turn_id=active,provider_turn_id='provider-'+active))
        emit(dict(kind='content',managed_turn_id=active,provider_turn_id='provider-'+active,text='fixture stream'))
        if v['input']=='approve':
            emit(dict(kind='approval_required',approval_kind='command_execution',provider_item_id='fixture-item',action_fingerprint=metadata['fingerprint'],structure=metadata['structure'],action=dict(kind='command',command='printf fixture',cwd=workspace,parsed_actions=[])))
        if v['input']=='finish':
            emit(dict(kind='turn_completed',managed_turn_id=active,provider_turn_id='provider-'+active))
            active=None
    elif v['method']=='turn/interrupt' and active:
        emit(dict(kind='turn_interrupted',managed_turn_id=active))
        active=None
"#;
// Only this explicit test wrapper opts into the normalized fixture's synthetic safety
// proof. ProcessWorker itself cannot approve an unverified provider preflight.
struct FixtureWorker(ProcessWorker);
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
struct FixtureSpawner {
    workspace: PathBuf,
    tools: bool,
}
impl Spawner for FixtureSpawner {
    fn descendant_containment(&self) -> bool {
        self.tools
    }
    fn spawn(&mut self, _: &RuntimeRef, _: Option<&mut LaunchStaging>) -> ControlResult<Spawned> {
        let mut command = if self.tools {
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
        let action = super::operations::ProviderAction::Command {
            command: "printf fixture".into(),
            cwd: self.workspace.to_str().unwrap().into(),
            parsed_actions: vec![],
        };
        let (_, fingerprint, structure) = action.metadata("fixture-item")?;
        command
            .args(["-u", "-c", SCRIPT])
            .arg(&self.workspace)
            .arg(if self.tools {
                "codex_local_tools_v1"
            } else {
                "codex_inference_v1"
            })
            .arg(json!({"fingerprint":fingerprint,"structure":structure}).to_string());
        Ok(Spawned {
            worker: Box::new(FixtureWorker(ProcessWorker::spawn_with(
                &mut command,
                Grace {
                    eof: Duration::from_millis(20),
                    term: Duration::from_millis(20),
                    kill: Duration::from_millis(100),
                },
                if self.tools {
                    ContainmentPolicy::DescendantsContained
                } else {
                    ContainmentPolicy::DirectChildOnly
                },
            )?)),
        })
    }
}
type Transport = F8Transport<FixtureSpawner>;
fn id() -> String {
    crate::control::store::uuid().as_str().to_owned()
}
fn poll_until(t: &mut Transport, mut condition: impl FnMut(&Transport) -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while !condition(t) {
        t.poll(Instant::now()).unwrap();
        assert!(Instant::now() < until, "process fixture deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn call(t: &mut Transport, c: &mut UnixStream, method: &str, params: Value) -> Value {
    let mut request = json!({"schema_version":2,"request_id":id(),"method":method,"params":params});
    if method != "authority/hello" {
        request["authority_instance_id"] = json!(AUTH);
    }
    let bytes = serde_json::to_vec(&request).unwrap();
    c.write_all(&(bytes.len() as u32).to_be_bytes()).unwrap();
    c.write_all(&bytes).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut answer = Vec::new();
    loop {
        t.poll(Instant::now()).unwrap();
        let mut scratch = [0; 8192];
        match c.read(&mut scratch) {
            Ok(0) => panic!("transport closed"),
            Ok(n) => answer.extend_from_slice(&scratch[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
            Err(e) => panic!("{e}"),
        }
        if answer.len() >= 4 {
            let n = u32::from_be_bytes(answer[..4].try_into().unwrap()) as usize;
            if answer.len() == n + 4 {
                return unique_json(&answer[4..]).unwrap();
            }
        }
        assert!(Instant::now() < deadline, "response timeout {method}");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn wire_error(value: &Value) -> Option<&str> {
    value["error"]["code"].as_str()
}

#[test]
fn owned_process_control_transport_binding_stream_cancel_relaunch_and_session_isolation() {
    let workspace = std::env::temp_dir().join(format!("wyss-f8-process-integration-{}", id()));
    std::fs::create_dir(&workspace).unwrap();
    let mut source = f8_fixture::v7_with_completed_turn();
    let conn = migration::prepare(&mut source).unwrap().into_connection();
    let authority = WireUuid::try_from(AUTH.to_owned()).unwrap();
    let engine = Engine::new(
        conn,
        authority.clone(),
        Registry::new(authority).unwrap().with_test_limit(2),
        None,
    )
    .unwrap();
    let mut transport = F8Transport::new(Service::new(
        Authority::new(engine),
        FixtureSpawner {
            workspace: workspace.clone(),
            tools: true,
        },
    ));
    let (owned, mut socket) = UnixStream::pair().unwrap();
    socket.set_nonblocking(true).unwrap();
    transport.attach(owned, Instant::now()).unwrap();
    assert!(
        call(
            &mut transport,
            &mut socket,
            "authority/hello",
            json!({"protocol_version":2,"client_instance_id":id()})
        )["result"]
            .is_object()
    );
    let mut sessions = Vec::new();
    let mut fences = Vec::new();
    for _ in 0..2 {
        let prepared = call(
            &mut transport,
            &mut socket,
            "managedSession/prepare",
            json!({"intent_id":id(),"adapter_id":"codex","runtime_kind":"codex"}),
        );
        let session = prepared["result"]["value"]["managed_session_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let start = call(
            &mut transport,
            &mut socket,
            "runtime/start",
            json!({"intent_id":id(),"adapter_id":"codex","runtime_kind":"codex","managed_session_id":session,"expected_revision":0,"expected_generation":0,"safety_profile":"codex_local_tools_v1","workspace_root":workspace}),
        );
        assert_eq!(wire_error(&start), None, "{start}");
        poll_until(&mut transport, |t| {
            t.service()
                .authority()
                .engine()
                .runtime_view(&session)
                .is_some_and(|(state, _)| state == "ready")
        });
        let reference = transport
            .service()
            .authority()
            .engine()
            .reference(&session)
            .unwrap();
        let fence = json!(reference.fence());
        let lease = call(
            &mut transport,
            &mut socket,
            "executionLease/grant",
            json!({"intent_id":id(),"session_id":session,"expected_session_revision":1,
            "scopes":["session_create","session_resume","turn_start","command_execute","file_change"],"ttl_seconds":300,"runtime_fence":fence}),
        );
        assert_eq!(wire_error(&lease), None, "{lease}");
        let intent = id();
        let params = json!({"intent_id":intent,"managed_session_id":session,"expected_session_revision":1,"workspace_root":workspace,"runtime_fence":fence});
        assert_eq!(
            wire_error(&call(
                &mut transport,
                &mut socket,
                "managedSession/runtime/create",
                params.clone()
            )),
            Some("runtime_operation_in_progress")
        );
        poll_until(&mut transport, |t| {
            t.service()
                .authority()
                .engine()
                .connection()
                .query_row(
                    "SELECT count(*) FROM managed_session_bindings WHERE managed_session_id=?1",
                    [&session],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap()
                > 0
        });
        let replay = call(
            &mut transport,
            &mut socket,
            "managedSession/runtime/create",
            params,
        );
        assert_eq!(wire_error(&replay), None, "{replay}");
        sessions.push(session);
        fences.push(fence);
    }
    let mut turns = Vec::new();
    for index in 0..2 {
        let turn = call(
            &mut transport,
            &mut socket,
            "managedSession/turn/start",
            json!({"intent_id":id(),"managed_session_id":sessions[index],"expected_session_revision":1,"prompt":if index==0 {"approve"} else {"wait"},"runtime_fence":fences[index]}),
        );
        assert_eq!(wire_error(&turn), None, "{turn}");
        turns.push(
            turn["result"]["value"]["managed_turn_id"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
    }
    poll_until(&mut transport, |t| {
        t.service()
            .authority()
            .engine()
            .connection()
            .query_row(
                "SELECT count(*) FROM managed_turns WHERE state='running'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
            == 2
    });
    poll_until(&mut transport, |t| {
        t.service()
            .authority()
            .engine()
            .connection()
            .query_row(
                "SELECT count(*) FROM managed_approvals WHERE state='pending'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
            == 1
    });
    let approval: String = transport
        .service()
        .authority()
        .engine()
        .connection()
        .query_row(
            "SELECT managed_approval_id FROM managed_approvals WHERE state='pending'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let answer = call(
        &mut transport,
        &mut socket,
        "managedTurn/approval/respond",
        json!({"intent_id":id(),"managed_approval_id":approval,"expected_revision":1,"decision":"accept","runtime_fence":fences[0]}),
    );
    assert_eq!(wire_error(&answer), None, "{answer}");
    poll_until(&mut transport, |t| {
        t.service()
            .authority()
            .engine()
            .connection()
            .query_row(
                "SELECT effect FROM managed_approvals WHERE managed_approval_id=?1",
                [&approval],
                |r| r.get::<_, String>(0),
            )
            .unwrap()
            == "confirmed"
    });
    let stream = call(
        &mut transport,
        &mut socket,
        "managedSession/turn/events",
        json!({"managed_turn_id":turns[0],"after_sequence":null,"limit":50}),
    );
    assert!(stream.to_string().contains("fixture stream"), "{stream}");
    let cross = call(
        &mut transport,
        &mut socket,
        "managedSession/turn/cancel",
        json!({"intent_id":id(),"managed_turn_id":turns[0],"expected_revision":2,"runtime_fence":fences[1]}),
    );
    assert_eq!(wire_error(&cross), Some("stale_revision"), "{cross}");
    let cancel = call(
        &mut transport,
        &mut socket,
        "managedSession/turn/cancel",
        json!({"intent_id":id(),"managed_turn_id":turns[0],"expected_revision":2,"runtime_fence":fences[0]}),
    );
    assert_eq!(wire_error(&cancel), None, "{cancel}");
    poll_until(&mut transport, |t| {
        t.service()
            .authority()
            .engine()
            .active_turn(&sessions[0])
            .is_none()
    });
    assert_eq!(
        transport
            .service()
            .authority()
            .engine()
            .active_turn(&sessions[1]),
        Some(turns[1].as_str())
    );
    // Owned lifecycle termination never touches the second runtime.
    let stop = call(
        &mut transport,
        &mut socket,
        "runtime/stop",
        json!({"intent_id":id(),"adapter_id":"codex","runtime_kind":"codex","managed_session_id":sessions[0],"expected_revision":1,"expected_generation":1,"expected_runtime_instance_id":fences[0]["runtime_instance_id"]}),
    );
    assert_eq!(wire_error(&stop), None, "{stop}");
    poll_until(&mut transport, |t| {
        t.service()
            .authority()
            .engine()
            .reference(&sessions[0])
            .is_none()
    });
    let restart = call(
        &mut transport,
        &mut socket,
        "runtime/start",
        json!({"intent_id":id(),"adapter_id":"codex","runtime_kind":"codex","managed_session_id":sessions[0],"expected_revision":2,"expected_generation":1,"safety_profile":"codex_local_tools_v1","workspace_root":workspace}),
    );
    assert_eq!(restart["result"]["value"]["generation"], 2, "{restart}");
    poll_until(&mut transport, |t| {
        t.service()
            .authority()
            .engine()
            .runtime_view(&sessions[0])
            .is_some_and(|(state, _)| state == "ready")
    });
    assert_eq!(
        transport
            .service()
            .authority()
            .engine()
            .active_turn(&sessions[1]),
        Some(turns[1].as_str())
    );
    // Drop moves remaining owned handles to the bounded reaper; stdin EOF stops fixtures.
    drop(transport);
    drop(socket);
    std::fs::remove_dir(&workspace).unwrap();
}
