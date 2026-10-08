//! Developer-only client. Fixed control operations; runtime process readiness never authorizes work.
use wyss_console::{
    contracts::{
        WireUuid,
        agent_adapter::{AgentAdapterId, AgentRuntimeKind},
        control::*,
        identities::{ExecutionLeaseId, ManagedApprovalId, ManagedSessionId, ManagedTurnId},
    },
    control::client::ControlClient,
};
fn id() -> WireUuid {
    WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap()
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mut c = ControlClient::connect()?;
    let result=match args.first().map(String::as_str) {
        Some("managedSession/turn/start") if args.len()==4=> {
            // Read content from stdin, never command-line argv or a durable request file.
            use std::io::Read;
            let mut prompt=String::new();std::io::stdin().take(8193).read_to_string(&mut prompt)?;
            serde_json::to_value(c.turn_start(TurnStart {intent_id:WireUuid::try_from(args[3].clone())?,managed_session_id:ManagedSessionId::parse(&args[1])?,expected_session_revision:args[2].parse()?,prompt})?)?
        },
        Some("managedSession/turn/status") if args.len()==2=>serde_json::to_value(c.turn_status_projection(ManagedTurnId::parse(&args[1])?)?)?,
        Some("managedSession/turn/events") if args.len()==4=>serde_json::to_value(c.turn_events(TurnEvents {managed_turn_id:ManagedTurnId::parse(&args[1])?,after_sequence:if args[2]=="0" {None} else {Some(args[2].parse()?)},limit:args[3].parse()?})?)?,
        Some("managedSession/turn/cancel") if args.len()==4=>serde_json::to_value(c.turn_cancel(TurnCancel {intent_id:WireUuid::try_from(args[3].clone())?,managed_turn_id:ManagedTurnId::parse(&args[1])?,expected_revision:args[2].parse()?})?)?,
        Some("managedTurn/approval/pending") if args.len()==2=>serde_json::to_value(c.approval_pending(ManagedTurnId::parse(&args[1])?)?)?,
        Some("managedTurn/approval/status") if args.len()==2||args.len()==4=>serde_json::to_value(c.approval_status(ApprovalGet{managed_approval_id:ManagedApprovalId::parse(&args[1])?,file_index:args.get(2).map(|s|s.parse()).transpose()?,offset:args.get(3).map(|s|s.parse()).transpose()?})?)?,
        // The decision is an intent: the authority derives and revalidates everything else.
        Some("managedTurn/approval/respond") if args.len()==5=>serde_json::to_value(c.approval_respond(ApprovalRespond{intent_id:WireUuid::try_from(args[4].clone())?,managed_approval_id:ManagedApprovalId::parse(&args[1])?,expected_revision:args[2].parse()?,decision:serde_json::from_value(serde_json::json!(args[3]))?})?)?,
        Some("managedSession/runtime/create") if args.len()==5=>serde_json::to_value(c.binding_create(BindingCreate{intent_id:WireUuid::try_from(args[4].clone())?,managed_session_id:ManagedSessionId::parse(&args[1])?,expected_session_revision:args[2].parse()?,workspace_root:args[3].clone()})?)?,
        Some("managedSession/runtime/resume") if args.len()==4=>serde_json::to_value(c.binding_resume(BindingResume{intent_id:WireUuid::try_from(args[3].clone())?,managed_session_id:ManagedSessionId::parse(&args[1])?,expected_session_revision:args[2].parse()?})?)?,
        Some("managedSession/runtime/status") if args.len()==2=>serde_json::to_value(c.binding_status(ManagedSessionId::parse(&args[1])?)?)?,
        Some("managedSession/runtime/abandon") if args.len()==4=>serde_json::to_value(c.binding_abandon(BindingAbandon{intent_id:WireUuid::try_from(args[3].clone())?,managed_session_id:ManagedSessionId::parse(&args[1])?,operation_id:WireUuid::try_from(args[2].clone())?})?)?,
        Some("executionLease/grant") if args.len()==6=>serde_json::to_value(c.lease_grant(LeaseGrant{intent_id:WireUuid::try_from(args[5].clone())?,session_id:ManagedSessionId::parse(&args[1])?,expected_session_revision:args[2].parse()?,ttl_seconds:args[3].parse()?,scopes:args[4].split(',').map(|s|serde_json::from_value(serde_json::json!(s))).collect::<Result<Vec<_>,_>>()?})?)?,
        Some("executionLease/get") if args.len()==2=>serde_json::to_value(c.lease_get(ExecutionLeaseId::parse(&args[1])?)?)?,
        Some("executionLease/active") if args.len()==2=>serde_json::to_value(c.lease_active(ManagedSessionId::parse(&args[1])?)?)?,
        Some("executionLease/renew") if args.len()==5=>serde_json::to_value(c.lease_renew(LeaseRenew{intent_id:WireUuid::try_from(args[4].clone())?,lease_id:ExecutionLeaseId::parse(&args[1])?,expected_revision:args[2].parse()?,ttl_seconds:args[3].parse()?})?)?,
        Some("executionLease/revoke") if args.len()==4=>serde_json::to_value(c.lease_revoke(LeaseRevoke{intent_id:WireUuid::try_from(args[3].clone())?,lease_id:ExecutionLeaseId::parse(&args[1])?,expected_revision:args[2].parse()?})?)?,
        Some("executionGate/status") if args.len()==3=>serde_json::to_value(c.gate_status(GateQuery{session_id:ManagedSessionId::parse(&args[1])?,action:serde_json::from_value(serde_json::json!(args[2]))?})?)?,
        Some("runtime/status") if args.len()==1=>serde_json::to_value(c.runtime_status()?)?,
        Some("runtime/model/list") if args.len()==1=>serde_json::to_value(c.runtime_models(RuntimeModels {adapter_id:AgentAdapterId::parse("codex")?,runtime_kind:AgentRuntimeKind::Codex,cursor:None,limit:20})?)?,
        Some("runtime/start"|"runtime/stop") if args.len()<=2=>{
            let p=RuntimeMutation{intent_id:if let Some(i)=args.get(1){WireUuid::try_from(i.clone())?}else{id()},adapter_id:AgentAdapterId::parse("codex")?,runtime_kind:AgentRuntimeKind::Codex,expected_revision:None,safety_profile:None,workspace_root:None};
            serde_json::to_value(if args[0]=="runtime/start" {c.runtime_start(p)?} else {c.runtime_stop(p)?})?
        },
        Some("runtime/start/local-tools") if (2..=3).contains(&args.len())=>serde_json::to_value(c.runtime_start(RuntimeMutation{intent_id:if let Some(i)=args.get(2){WireUuid::try_from(i.clone())?}else{id()},adapter_id:AgentAdapterId::parse("codex")?,runtime_kind:AgentRuntimeKind::Codex,expected_revision:None,safety_profile:Some(RuntimeSafetyProfile::CodexLocalToolsV1),workspace_root:Some(args[1].clone())})?)?,
        Some("hello") if args.len()==1=>serde_json::to_value(c.hello_status())?,
        Some("status") if args.len()==1=>serde_json::to_value(c.status()?)?,
        Some("prepare") if args.len()<=2=>serde_json::to_value(c.prepare(Prepare{intent_id:if let Some(i)=args.get(1){WireUuid::try_from(i.clone())?}else{id()},adapter_id:AgentAdapterId::parse("codex")?,runtime_kind:AgentRuntimeKind::Codex})?)?,
        Some("get") if args.len()==2=>serde_json::to_value(c.get(ManagedSessionId::parse(&args[1])?)?)?,
        Some("list") if args.len()==1=>serde_json::to_value(c.list(List{after:None,limit:50})?)?,
        Some("audit") if args.len()<=3=>serde_json::to_value(c.audit(AuditRead{stream_id:c.hello_status().stream_id.clone(),after_sequence:args.get(1).filter(|s|*s!="0").map(|s|s.parse()).transpose()?,limit:args.get(2).map_or(Ok(50),|s|s.parse())?})?)?,
        // The cursor is the caller's own: the authority keeps no position for any client.
        Some("audit/watch") if args.len()==4=>serde_json::to_value(c.audit_watch(AuditWatch{stream_id:c.hello_status().stream_id.clone(),after_sequence:Some(&args[1]).filter(|s|*s!="0").map(|s|s.parse()).transpose()?,limit:args[2].parse()?,wait_ms:args[3].parse()?})?)?,
        Some("close") if args.len()==3=>serde_json::to_value(c.close(Close{intent_id:id(),managed_session_id:ManagedSessionId::parse(&args[1])?,expected_revision:args[2].parse()?})?)?,
        _=>return Err("usage: control_client managedSession/turn/start session revision intent (prompt on stdin)|managedSession/turn/status turn|managedSession/turn/events turn after_sequence limit|managedSession/turn/cancel turn revision intent|managedTurn/approval/pending turn|managedTurn/approval/status approval [file_index offset]|managedTurn/approval/respond approval revision accept|decline|cancel intent|managedSession/runtime/create session revision workspace intent|managedSession/runtime/status session|managedSession/runtime/resume session revision intent|managedSession/runtime/abandon session operation intent|executionLease/grant session revision ttl scopes_csv intent|executionLease/get lease|executionLease/active session|executionLease/renew lease revision ttl intent|executionLease/revoke lease revision intent|executionGate/status session action|hello|status|prepare [intent UUID]|get UUID|list|audit [after_sequence|0 [limit]]|audit/watch after_sequence|0 limit wait_ms|close UUID revision|runtime/status|runtime/model/list|runtime/start [intent UUID]|runtime/start/local-tools workspace [intent UUID]|runtime/stop [intent UUID]".into()),
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("control_client: {e}");
        std::process::exit(1);
    }
}
