//! Developer-only client. Fixed minimal control operations, never runtime execution.
use wyss_console::{
    contracts::{
        WireUuid,
        agent_adapter::{AgentAdapterId, AgentRuntimeKind},
        control::*,
        identities::ManagedSessionId,
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
        Some("hello") if args.len()==1=>serde_json::to_value(c.hello_status())?,
        Some("status") if args.len()==1=>serde_json::to_value(c.status()?)?,
        Some("prepare") if args.len()<=2=>serde_json::to_value(c.prepare(Prepare{intent_id:if let Some(i)=args.get(1){WireUuid::try_from(i.clone())?}else{id()},adapter_id:AgentAdapterId::parse("codex")?,runtime_kind:AgentRuntimeKind::Codex})?)?,
        Some("get") if args.len()==2=>serde_json::to_value(c.get(ManagedSessionId::parse(&args[1])?)?)?,
        Some("list") if args.len()==1=>serde_json::to_value(c.list(List{after:None,limit:50})?)?,
        Some("audit") if args.len()==1=>serde_json::to_value(c.audit(AuditRead{stream_id:c.hello_status().stream_id.clone(),after_sequence:None,limit:50})?)?,
        Some("close") if args.len()==3=>serde_json::to_value(c.close(Close{intent_id:id(),managed_session_id:ManagedSessionId::parse(&args[1])?,expected_revision:args[2].parse()?})?)?,
        _=>return Err("usage: control_client hello|status|prepare [intent UUID]|get UUID|list|audit|close UUID revision".into()),
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
