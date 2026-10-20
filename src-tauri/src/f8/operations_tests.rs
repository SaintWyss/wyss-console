use super::*;
use crate::f8::{migration,registry::Registry,engine::{Spawner,Spawned,Worker,Attachment}};
use std::{sync::{Arc,Mutex}};
const AUTH:&str="00000000-0000-4000-8000-0000000000a1";
fn uuid()->WireUuid{crate::control::store::uuid()}
fn request(method:&str,params:Value)->Request{Request::parse(&serde_json::to_vec(&json!({"schema_version":2,"request_id":uuid(),"authority_instance_id":AUTH,"method":method,"params":params})).unwrap()).unwrap()}
fn rig()->Engine{let mut source=crate::control::f8_fixture::v7_with_sessions(1);let conn=migration::prepare(&mut source).unwrap().into_connection();let auth=WireUuid::try_from(AUTH.to_owned()).unwrap();Engine::new(conn,auth.clone(),Registry::new(auth).unwrap().with_test_limit(2),None).unwrap()}
fn call(e:&mut Engine,r:&Request)->ControlResult<Value>{handle(e,r,&mut Streams::default())}
fn prepare(e:&mut Engine)->String{call(e,&request("managedSession/prepare",json!({"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex"}))).unwrap()["value"]["managed_session_id"].as_str().unwrap().to_owned()}
fn head(e:&Engine)->String{e.connection().query_row("SELECT last_sequence FROM authority_metadata WHERE id=1",[],|r|r.get(0)).unwrap()}
#[test]
fn session_prepare_replay_close_fence_and_reopen_validation(){
    let mut e=rig();let id=uuid();let p=request("managedSession/prepare",json!({"intent_id":id,"adapter_id":"codex","runtime_kind":"codex"}));
    let first=call(&mut e,&p).unwrap();let h=head(&e);assert_eq!(call(&mut e,&p).unwrap(),first);assert_eq!(head(&e),h);
    let sid=first["value"]["managed_session_id"].as_str().unwrap();
    let f=json!({"managed_session_id":sid,"generation":0,"runtime_instance_id":null});
    let close=request("managedSession/close",json!({"intent_id":uuid(),"managed_session_id":sid,"expected_revision":1,"runtime_fence":f}));
    assert_eq!(call(&mut e,&close).unwrap()["value"]["control_state"],"closed");let h=head(&e);call(&mut e,&close).unwrap();assert_eq!(head(&e),h);
    super::super::v8::validate_v8(e.connection()).unwrap();
}
#[test]
fn stale_close_never_mutates_audit(){
    let mut e=rig();let sid=prepare(&mut e);let h=head(&e);
    let close=request("managedSession/close",json!({"intent_id":uuid(),"managed_session_id":sid,"expected_revision":1,"runtime_fence":{"managed_session_id":sid,"generation":1,"runtime_instance_id":null}}));
    assert_eq!(call(&mut e,&close),Err(E::StaleRevision));assert_eq!(head(&e),h);
}
#[test]
fn session_pagination_is_durable_ordered_and_bounded(){
    let mut e=rig();prepare(&mut e);let first=call(&mut e,&request("managedSession/list",json!({"after":null,"limit":1}))).unwrap();
    assert_eq!(first["value"]["sessions"].as_array().unwrap().len(),1);let next=first["value"]["next"].clone();assert!(!next.is_null());
    let second=call(&mut e,&request("managedSession/list",json!({"after":next,"limit":1}))).unwrap();assert_ne!(first["value"]["sessions"][0],second["value"]["sessions"][0]);
}
#[test]
fn legacy_turn_reads_remain_unassigned_and_stream_gap_is_explicit(){
    let mut e=rig();let id:String=e.connection().query_row("SELECT managed_turn_id FROM managed_turns LIMIT 1",[],|r|r.get(0)).unwrap();
    let status=call(&mut e,&request("managedSession/turn/status",json!({"managed_turn_id":id}))).unwrap();
    assert!(status["value"]["runtime_ref"].is_null());assert!(status["value"]["attestation"].is_null());assert_eq!(status["value"]["turn"]["state"],"completed");
    let events=call(&mut e,&request("managedSession/turn/events",json!({"managed_turn_id":id,"after_sequence":null,"limit":1}))).unwrap();assert_eq!(events["value"]["retained"],false);assert_eq!(events["value"]["gap"],true);assert_eq!(events["value"]["events"][0]["kind"],"stream_gap");
}
struct Recording(Arc<Mutex<Vec<Vec<u8>>>>);
impl Worker for Recording{fn preflight(&mut self,_:&Attachment)->ControlResult<()>{Ok(())}fn send(&mut self,b:&[u8])->ControlResult<()>{self.0.lock().unwrap().push(b.to_vec());Ok(())}}
struct Spawn(Arc<Mutex<Vec<Vec<u8>>>>);
impl Spawner for Spawn{fn spawn(&mut self,_:&RuntimeRef,_:Option<&mut super::super::staging::LaunchStaging>)->ControlResult<Spawned>{Ok(Spawned{worker:Box::new(Recording(self.0.clone()))})}}
fn ready(e:&mut Engine,sid:&str,frames:Arc<Mutex<Vec<Vec<u8>>>>)->RuntimeRef{
    e.start(&request("runtime/start",json!({"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex","managed_session_id":sid,"expected_generation":0,"expected_revision":0})),&mut Spawn(frames)).unwrap();
    let reference=e.reference(sid).unwrap();e.complete_handshake(&reference,"0.159.0",Attachment{workspace:"/tmp".into(),profile:"codex_inference_v1".into(),revision:1}).unwrap();reference
}
#[test]
fn binding_replay_dispatches_once_and_valid_foreign_runtime_cannot_confirm(){
    let mut e=rig();let a=prepare(&mut e);let b=prepare(&mut e);let frames:Arc<Mutex<Vec<Vec<u8>>>>=Arc::default();let ra=ready(&mut e,&a,frames.clone());let rb=ready(&mut e,&b,Arc::default());
    for r in [&ra,&rb]{super::super::writers::Writer::new(e.connection_mut(),WireUuid::try_from(AUTH.to_owned()).unwrap()).unwrap().lease(&v1::Operation::LeaseGrant(v1::LeaseGrant{intent_id:uuid(),session_id:r.managed_session_id.clone(),expected_session_revision:1,scopes:vec![v1::ExecutionScope::SessionCreate],ttl_seconds:3600})).unwrap();}
    let intent=uuid();let op=request("managedSession/runtime/create",json!({"intent_id":intent,"managed_session_id":a,"expected_session_revision":1,"workspace_root":"/tmp","runtime_fence":ra.fence()}));
    assert_eq!(call(&mut e,&op),Err(E::RuntimeOperationInProgress));assert_eq!(call(&mut e,&op),Err(E::RuntimeOperationInProgress));assert_eq!(frames.lock().unwrap().len(),1);
    let h=head(&e);assert_eq!(confirm_binding(&mut e,&rb,&intent,"thread-a"),Err(E::InvalidRequest));assert_eq!(head(&e),h);
    confirm_binding(&mut e,&ra,&intent,"thread-a").unwrap();assert_eq!(call(&mut e,&op).unwrap()["kind"],"binding");assert_eq!(frames.lock().unwrap().len(),1);
    super::super::v8::validate_v8(e.connection()).unwrap();
}
#[test]
fn recover_pending_create_is_repeatable_and_never_redispatches(){
    let mut e=rig();let sid=prepare(&mut e);let frames:Arc<Mutex<Vec<Vec<u8>>>>=Arc::default();let r=ready(&mut e,&sid,frames.clone());
    super::super::writers::Writer::new(e.connection_mut(),WireUuid::try_from(AUTH.to_owned()).unwrap()).unwrap().lease(&v1::Operation::LeaseGrant(v1::LeaseGrant{intent_id:uuid(),session_id:r.managed_session_id.clone(),expected_session_revision:1,scopes:vec![v1::ExecutionScope::SessionCreate],ttl_seconds:3600})).unwrap();
    let op=request("managedSession/runtime/create",json!({"intent_id":uuid(),"managed_session_id":sid,"expected_session_revision":1,"workspace_root":"/tmp","runtime_fence":r.fence()}));assert_eq!(call(&mut e,&op),Err(E::RuntimeOperationInProgress));
    assert_eq!(recover_bindings(&mut e).unwrap(),0);
    e.transport_failed(&r);e.poll(std::time::Instant::now());
    assert_eq!(recover_bindings(&mut e).unwrap(),1);let h=head(&e);assert_eq!(recover_bindings(&mut e).unwrap(),0);assert_eq!(head(&e),h);assert_eq!(call(&mut e,&op),Err(E::BindingIndeterminate));assert_eq!(frames.lock().unwrap().len(),1);
}
