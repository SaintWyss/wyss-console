//! Wyss Control Protocol v1. Runtime supervision does not authorize agent work.
pub use super::approval::*;
pub use super::binding::*;
pub use super::execution::*;
pub use super::turn::*;
use super::{
    Sequence, UtcTimestamp, WireUuid,
    agent_adapter::{AgentAdapterId, AgentRuntimeKind},
    event_envelope::EventEnvelopeV1,
    identities::ManagedSessionId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

pub const PROTOCOL_VERSION: u8 = 1;
pub const MAX_FRAME: usize = 65_536;
pub const MAX_PAGE: u16 = 50;
/// Longest a durable-event watch may wait for a commit before answering empty.
pub const MAX_WATCH_MS: u32 = 30_000;
pub const METHODS: [&str; 29] = [
    "managedSession/turn/start",
    "managedSession/turn/status",
    "managedSession/turn/events",
    "managedSession/turn/cancel",
    "managedSession/runtime/create",
    "managedSession/runtime/resume",
    "managedSession/runtime/status",
    "managedSession/runtime/abandon",
    "authority/hello",
    "authority/status",
    "managedSession/prepare",
    "managedSession/get",
    "managedSession/list",
    "managedSession/close",
    "audit/read",
    "audit/watch",
    "runtime/start",
    "runtime/status",
    "runtime/model/list",
    "runtime/stop",
    "executionLease/grant",
    "executionLease/get",
    "executionLease/active",
    "executionLease/renew",
    "executionLease/revoke",
    "executionGate/status",
    "managedTurn/approval/pending",
    "managedTurn/approval/status",
    "managedTurn/approval/respond",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlError {
    ApprovalNotFound,
    TurnNotFound,
    ActiveTurnExists,
    ProtocolVersionUnsupported,
    HandshakeRequired,
    InvalidRequest,
    UnsupportedMethod,
    Unsupported,
    FrameTooLarge,
    TruncatedFrame,
    Timeout,
    PeerRejected,
    UnsafePath,
    AlreadyRunning,
    StorageUnavailable,
    StorageIncompatible,
    SessionNotFound,
    LeaseNotFound,
    LeaseAlreadyActive,
    AuthorityUnavailable,
    InvalidTransition,
    StaleRevision,
    IntentConflict,
    BindingNotFound,
    BindingAlreadyExists,
    BindingIndeterminate,
    WorkspaceInvalid,
    RuntimeOperationInProgress,
    ExternalEffectUnknown,
    GateDenied,
    ProviderRejected,
    CapacityExceeded,
    InternalError,
    TransportError,
    CorrelationError,
}
impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = serde_json::to_value(self).map_err(|_| fmt::Error)?;
        f.write_str(value.as_str().ok_or(fmt::Error)?)
    }
}
impl std::error::Error for ControlError {}
pub type ControlResult<T> = Result<T, ControlError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub protocol_version: u8,
    pub client_instance_id: WireUuid,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Empty {}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prepare {
    pub intent_id: WireUuid,
    pub adapter_id: AgentAdapterId,
    pub runtime_kind: AgentRuntimeKind,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Get {
    pub managed_session_id: ManagedSessionId,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct List {
    pub after: Option<Sequence>,
    pub limit: u16,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Close {
    pub intent_id: WireUuid,
    pub managed_session_id: ManagedSessionId,
    pub expected_revision: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditRead {
    pub stream_id: WireUuid,
    pub after_sequence: Option<Sequence>,
    pub limit: u16,
}
/// The same bounded read as `audit/read`, answered at once when history exists after the
/// cursor and otherwise after the next durable commit or `wait_ms`, whichever is first.
/// The cursor belongs to the client: the authority keeps no subscriber state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditWatch {
    pub stream_id: WireUuid,
    pub after_sequence: Option<Sequence>,
    pub limit: u16,
    pub wait_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", deny_unknown_fields)]
pub enum Operation {
    #[serde(rename = "managedSession/turn/start")]
    TurnStart(TurnStart),
    #[serde(rename = "managedSession/turn/status")]
    TurnStatus(TurnGet),
    #[serde(rename = "managedSession/turn/events")]
    TurnEvents(TurnEvents),
    #[serde(rename = "managedSession/turn/cancel")]
    TurnCancel(TurnCancel),
    #[serde(rename = "managedSession/runtime/create")]
    BindingCreate(BindingCreate),
    #[serde(rename = "managedSession/runtime/resume")]
    BindingResume(BindingResume),
    #[serde(rename = "managedSession/runtime/status")]
    BindingStatus(Get),
    #[serde(rename = "managedSession/runtime/abandon")]
    BindingAbandon(BindingAbandon),
    #[serde(rename = "authority/hello")]
    Hello(Hello),
    #[serde(rename = "authority/status")]
    Status(Empty),
    #[serde(rename = "managedSession/prepare")]
    Prepare(Prepare),
    #[serde(rename = "managedSession/get")]
    Get(Get),
    #[serde(rename = "managedSession/list")]
    List(List),
    #[serde(rename = "managedSession/close")]
    Close(Close),
    #[serde(rename = "runtime/start")]
    RuntimeStart(RuntimeMutation),
    #[serde(rename = "runtime/stop")]
    RuntimeStop(RuntimeMutation),
    #[serde(rename = "runtime/status")]
    RuntimeStatus(RuntimeTarget),
    #[serde(rename = "runtime/model/list")]
    RuntimeModels(RuntimeModels),
    #[serde(rename = "audit/read")]
    AuditRead(AuditRead),
    #[serde(rename = "audit/watch")]
    AuditWatch(AuditWatch),
    #[serde(rename = "executionLease/grant")]
    LeaseGrant(LeaseGrant),
    #[serde(rename = "executionLease/get")]
    LeaseGet(LeaseGet),
    #[serde(rename = "executionLease/active")]
    LeaseActive(LeaseActive),
    #[serde(rename = "executionLease/renew")]
    LeaseRenew(LeaseRenew),
    #[serde(rename = "executionLease/revoke")]
    LeaseRevoke(LeaseRevoke),
    #[serde(rename = "executionGate/status")]
    GateStatus(GateQuery),
    #[serde(rename = "managedTurn/approval/pending")]
    ApprovalPending(ApprovalPending),
    #[serde(rename = "managedTurn/approval/status")]
    ApprovalStatus(ApprovalGet),
    #[serde(rename = "managedTurn/approval/respond")]
    ApprovalRespond(ApprovalRespond),
}
impl Operation {
    pub fn validate(&self) -> ControlResult<()> {
        match self {
            Self::TurnStart(p) => {
                valid_revision(p.expected_session_revision)?;
                validate_prompt(&p.prompt)
            }
            Self::TurnCancel(p) => valid_revision(p.expected_revision),
            Self::RuntimeStart(p) => p.validate(true),
            Self::RuntimeStop(p) => p.validate(false),
            Self::ApprovalRespond(p) => valid_revision(p.expected_revision),
            Self::TurnEvents(p) if p.limit == 0 || p.limit > 50 => {
                Err(ControlError::InvalidRequest)
            }
            Self::BindingCreate(p) => {
                valid_revision(p.expected_session_revision)?;
                validate_workspace_text(&p.workspace_root)
            }
            Self::BindingResume(p) => valid_revision(p.expected_session_revision),
            Self::LeaseGrant(p) => {
                validate_scopes(&p.scopes)?;
                validate_ttl(p.ttl_seconds)?;
                valid_revision(p.expected_session_revision)
            }
            Self::LeaseRenew(p) => {
                validate_ttl(p.ttl_seconds)?;
                valid_revision(p.expected_revision)
            }
            Self::LeaseRevoke(p) => valid_revision(p.expected_revision),
            Self::Hello(h) if h.protocol_version != PROTOCOL_VERSION => {
                Err(ControlError::ProtocolVersionUnsupported)
            }
            Self::List(p) if p.limit == 0 || p.limit > MAX_PAGE => {
                Err(ControlError::InvalidRequest)
            }
            Self::AuditRead(p) if p.limit == 0 || p.limit > MAX_PAGE => {
                Err(ControlError::InvalidRequest)
            }
            Self::AuditWatch(p)
                if p.limit == 0 || p.limit > MAX_PAGE || p.wait_ms > MAX_WATCH_MS =>
            {
                Err(ControlError::InvalidRequest)
            }
            Self::Close(p) if !(1..=2).contains(&p.expected_revision) => {
                Err(ControlError::InvalidRequest)
            }
            Self::RuntimeModels(p) if p.limit == 0 || p.limit > 20 => {
                Err(ControlError::InvalidRequest)
            }
            _ => Ok(()),
        }
    }
}
#[derive(Debug, Clone)]
pub struct Request {
    pub request_id: WireUuid,
    pub operation: Operation,
}
impl Request {
    pub fn encode(&self) -> ControlResult<Vec<u8>> {
        self.operation.validate()?;
        let mut v =
            serde_json::to_value(&self.operation).map_err(|_| ControlError::InvalidRequest)?;
        v["schema_version"] = Value::from(PROTOCOL_VERSION);
        v["request_id"] =
            serde_json::to_value(&self.request_id).map_err(|_| ControlError::InvalidRequest)?;
        encode_value(&v)
    }
    pub fn parse(bytes: &[u8]) -> ControlResult<Self> {
        let mut v = unique_json(bytes)?;
        exact(&v, &["schema_version", "request_id", "method", "params"])?;
        version(&v)?;
        let request_id = serde_json::from_value(v["request_id"].clone())
            .map_err(|_| ControlError::InvalidRequest)?;
        let method = v["method"].as_str().ok_or(ControlError::InvalidRequest)?;
        if !METHODS.contains(&method) {
            return Err(ControlError::UnsupportedMethod);
        }
        let o = v.as_object_mut().ok_or(ControlError::InvalidRequest)?;
        o.remove("schema_version");
        o.remove("request_id");
        let operation: Operation =
            serde_json::from_value(v).map_err(|_| ControlError::InvalidRequest)?;
        operation.validate()?;
        Ok(Self {
            request_id,
            operation,
        })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlState {
    Prepared,
    Closed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedSessionRecord {
    pub managed_session_id: ManagedSessionId,
    pub adapter_id: AgentAdapterId,
    pub runtime_kind: AgentRuntimeKind,
    pub control_state: ControlState,
    pub revision: u64,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
    pub created_sequence: Sequence,
}
impl ManagedSessionRecord {
    pub fn validate(&self) -> ControlResult<()> {
        if self.revision
            != match self.control_state {
                ControlState::Prepared => 1,
                ControlState::Closed => 2,
            }
        {
            return Err(ControlError::StorageIncompatible);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityStatus {
    pub protocol_version: u8,
    pub authority_instance_id: WireUuid,
    pub service_version: String,
    pub storage_schema_version: u8,
    pub stream_id: WireUuid,
    /// Null before the first committed event; otherwise positive decimal string.
    pub last_sequence: Option<Sequence>,
    pub prepared_count: u64,
    pub closed_count: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ResultValue {
    ApprovalReceipt(ApprovalReceipt),
    /// One approval: canonical metadata and, while it is held, its transient action.
    Approval(Box<ApprovalStatus>),
    /// The approval a turn is waiting on, if any.
    PendingApproval(Option<Box<ApprovalStatus>>),
    Turn(ManagedTurnRecord),
    TurnStatus(ManagedTurnStatus),
    TurnEvents(TurnStreamPage),
    Hello(AuthorityStatus),
    Status(AuthorityStatus),
    Session(ManagedSessionRecord),
    Binding(SessionBinding),
    BindingStatus(BindingStatus),
    BindingAbandoned(AbandonedOperation),
    Lease(ExecutionLease),
    ActiveLease(Option<ExecutionLease>),
    GateStatus(GateStatus),
    RuntimeControl(RuntimeControlRecord),
    RuntimeStatus(RuntimeStatus),
    RuntimeModels {
        runtime_instance_id: super::identities::AgentRuntimeInstanceId,
        models: Vec<super::agent_adapter::AgentModelDescriptorV1>,
        next: Option<ModelCursor>,
    },
    Sessions {
        sessions: Vec<ManagedSessionRecord>,
        next: Option<Sequence>,
    },
    Audit {
        events: Vec<EventEnvelopeV1>,
        next: Option<Sequence>,
    },
}
#[derive(Debug, Clone)]
pub struct Response {
    pub request_id: WireUuid,
    pub authority_instance_id: WireUuid,
    pub outcome: ControlResult<ResultValue>,
}
impl Response {
    pub fn encode(&self) -> ControlResult<Vec<u8>> {
        let mut value = serde_json::json!({"schema_version":1,"request_id":self.request_id,"authority_instance_id":self.authority_instance_id});
        match &self.outcome {
            Ok(r) => {
                value["result"] =
                    serde_json::to_value(r).map_err(|_| ControlError::InternalError)?
            }
            Err(e) => value["error"] = serde_json::json!({"code":e}),
        }
        encode_value(&value)
    }
    pub fn parse(bytes: &[u8]) -> ControlResult<Self> {
        let v = unique_json(bytes)?;
        if v.get("result").is_some() {
            exact(
                &v,
                &[
                    "schema_version",
                    "request_id",
                    "authority_instance_id",
                    "result",
                ],
            )?;
        } else {
            exact(
                &v,
                &[
                    "schema_version",
                    "request_id",
                    "authority_instance_id",
                    "error",
                ],
            )?;
        }
        version(&v)?;
        let request_id = serde_json::from_value(v["request_id"].clone())
            .map_err(|_| ControlError::InvalidRequest)?;
        let authority_instance_id = serde_json::from_value(v["authority_instance_id"].clone())
            .map_err(|_| ControlError::InvalidRequest)?;
        let outcome = if let Some(r) = v.get("result") {
            let r: ResultValue =
                serde_json::from_value(r.clone()).map_err(|_| ControlError::InvalidRequest)?;
            validate_result(&r)?;
            Ok(r)
        } else {
            exact(&v["error"], &["code"])?;
            Err(serde_json::from_value(v["error"]["code"].clone())
                .map_err(|_| ControlError::InvalidRequest)?)
        };
        Ok(Self {
            request_id,
            authority_instance_id,
            outcome,
        })
    }
}
fn validate_result(r: &ResultValue) -> ControlResult<()> {
    match r {
        ResultValue::Hello(s) | ResultValue::Status(s) => {
            if s.protocol_version != 1
                || s.storage_schema_version != 7
                || s.service_version.len() > 128
            {
                return Err(ControlError::InvalidRequest);
            }
        }
        ResultValue::Binding(b) => b.validate()?,
        ResultValue::BindingStatus(s) => s.validate()?,
        ResultValue::BindingAbandoned(_) => (),
        ResultValue::Lease(s) => s.validate()?,
        ResultValue::ActiveLease(s) => {
            if let Some(s) = s {
                s.validate()?;
                if s.state != LeaseState::Active {
                    return Err(ControlError::InvalidRequest);
                }
            }
        }
        ResultValue::GateStatus(_) => (),
        ResultValue::RuntimeControl(s) => s.validate()?,
        ResultValue::RuntimeStatus(s) => s.validate()?,
        ResultValue::Turn(t) => t.validate()?,
        ResultValue::ApprovalReceipt(r) => {
            r.validate().map_err(|_| ControlError::InvalidRequest)?
        }
        ResultValue::Approval(s) => s.validate()?,
        ResultValue::PendingApproval(s) => {
            if let Some(s) = s {
                s.validate()?;
                if s.approval.state != ApprovalState::Pending {
                    return Err(ControlError::InvalidRequest);
                }
            }
        }
        ResultValue::TurnStatus(s) => {
            s.turn.validate()?;
            if s.stream_earliest_sequence
                .zip(s.stream_latest_sequence)
                .is_some_and(|(a, b)| a > b)
            {
                return Err(ControlError::InvalidRequest);
            }
        }
        ResultValue::TurnEvents(p) => {
            if p.events.len() > 50
                || p.events.iter().any(|e| {
                    e.schema_version != 1 || e.text.as_ref().is_some_and(|t| t.len() > 4096)
                })
            {
                return Err(ControlError::InvalidRequest);
            }
        }
        ResultValue::RuntimeModels { models, .. } => {
            if models.len() > 20 {
                return Err(ControlError::InvalidRequest);
            }
        }
        ResultValue::Session(s) => s.validate()?,
        ResultValue::Sessions { sessions, .. } => {
            if sessions.len() > MAX_PAGE as usize {
                return Err(ControlError::InvalidRequest);
            }
            for s in sessions {
                s.validate()?;
            }
        }
        ResultValue::Audit { events, .. } => {
            if events.len() > MAX_PAGE as usize {
                return Err(ControlError::InvalidRequest);
            }
            for e in events {
                e.validate().map_err(|_| ControlError::InvalidRequest)?;
            }
        }
    }
    Ok(())
}
fn version(v: &Value) -> ControlResult<()> {
    if v["schema_version"].as_u64() != Some(1) {
        return Err(ControlError::ProtocolVersionUnsupported);
    }
    Ok(())
}
fn exact(v: &Value, names: &[&str]) -> ControlResult<()> {
    let o = v.as_object().ok_or(ControlError::InvalidRequest)?;
    if o.len() != names.len() || names.iter().any(|n| !o.contains_key(*n)) {
        return Err(ControlError::InvalidRequest);
    }
    Ok(())
}
fn encode_value(v: &Value) -> ControlResult<Vec<u8>> {
    let b = serde_json::to_vec(v).map_err(|_| ControlError::InvalidRequest)?;
    if b.len() > MAX_FRAME {
        Err(ControlError::FrameTooLarge)
    } else {
        Ok(b)
    }
}

/// Reject duplicate members at every depth before typed decoding can erase ambiguity.
/// The frame byte bound and serde's recursion limit bound this temporary value.
pub(crate) fn unique_json(bytes: &[u8]) -> ControlResult<Value> {
    if bytes.is_empty() {
        return Err(ControlError::InvalidRequest);
    }
    if bytes.len() > MAX_FRAME {
        return Err(ControlError::FrameTooLarge);
    }
    std::str::from_utf8(bytes).map_err(|_| ControlError::InvalidRequest)?;
    let mut d = serde_json::Deserializer::from_slice(bytes);
    let v = Unique::deserialize(&mut d)
        .map_err(|_| ControlError::InvalidRequest)?
        .0;
    d.end().map_err(|_| ControlError::InvalidRequest)?;
    Ok(v)
}
struct Unique(Value);
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Unique;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("unique JSON")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Unique, E> {
                Ok(Unique(serde_json::json!(v)))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut a: A) -> Result<Unique, A::Error> {
                let mut v = Vec::new();
                while let Some(x) = a.next_element::<Unique>()? {
                    v.push(x.0);
                }
                Ok(Unique(Value::Array(v)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut a: A) -> Result<Unique, A::Error> {
                let mut v = serde_json::Map::new();
                while let Some(k) = a.next_key::<String>()? {
                    if v.contains_key(&k) {
                        return Err(serde::de::Error::custom("duplicate member"));
                    }
                    v.insert(k, a.next_value::<Unique>()?.0);
                }
                Ok(Unique(Value::Object(v)))
            }
        }
        d.deserialize_any(V)
    }
}

// Runtime control has no conversational input, session binding or executable parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeTarget {
    pub adapter_id: AgentAdapterId,
    pub runtime_kind: AgentRuntimeKind,
}
impl RuntimeTarget {
    pub fn codex() -> Self {
        Self {
            adapter_id: AgentAdapterId::parse("codex").expect("fixed adapter"),
            runtime_kind: AgentRuntimeKind::Codex,
        }
    }
    pub fn supported(&self) -> ControlResult<()> {
        if self == &Self::codex() {
            Ok(())
        } else {
            Err(ControlError::Unsupported)
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeMutation {
    pub intent_id: WireUuid,
    pub adapter_id: AgentAdapterId,
    pub runtime_kind: AgentRuntimeKind,
    pub expected_revision: Option<u64>,
    /// Start only. Absent selects the inference profile. The profile of a generation never
    /// changes: another profile needs a stop and a fresh start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_profile: Option<RuntimeSafetyProfile>,
    /// Start only. The one workspace a local-tools generation is isolated for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
}
impl RuntimeMutation {
    /// A local-tools start names exactly one workspace; nothing else names any.
    pub fn validate(&self, start: bool) -> ControlResult<()> {
        let local = self.safety_profile == Some(RuntimeSafetyProfile::CodexLocalToolsV1);
        if (!start && self.safety_profile.is_some()) || local != self.workspace_root.is_some() {
            return Err(ControlError::InvalidRequest);
        }
        self.workspace_root
            .as_deref()
            .map_or(Ok(()), validate_workspace_text)
    }
    pub fn target(&self) -> RuntimeTarget {
        RuntimeTarget {
            adapter_id: self.adapter_id.clone(),
            runtime_kind: self.runtime_kind,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesiredState {
    Running,
    Stopped,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActualState {
    Stopped,
    Starting,
    Initializing,
    Ready,
    Stopping,
    Failed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeControlRecord {
    pub adapter_id: AgentAdapterId,
    pub runtime_kind: AgentRuntimeKind,
    pub desired_state: DesiredState,
    pub revision: u64,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}
impl RuntimeControlRecord {
    pub fn validate(&self) -> ControlResult<()> {
        RuntimeTarget {
            adapter_id: self.adapter_id.clone(),
            runtime_kind: self.runtime_kind,
        }
        .supported()?;
        if self.revision == 0 || self.revision > i64::MAX as u64 {
            return Err(ControlError::StorageIncompatible);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeDiagnostic {
    Unavailable,
    SpawnFailed,
    TransportFailed,
    HandshakeTimeout,
    ProtocolFailed,
    ServerRequest,
    RuntimeRejected,
    CatalogRejected,
    ChildExited,
    AuditUnavailable,
    InferenceIsolationViolation,
    SafetyUnproven,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeMethods {
    pub initialize: u8,
    pub initialized: u8,
    pub model_list: u8,
    #[serde(default)]
    pub thread_start: u64,
    #[serde(default)]
    pub thread_archive: u64,
    #[serde(default)]
    pub thread_unarchive: u64,
    #[serde(default)]
    pub thread_resume: u64,
    #[serde(default)]
    pub config_read: u64,
    #[serde(default)]
    pub hooks_list: u64,
    #[serde(default)]
    pub turn_start: u64,
    #[serde(default)]
    pub turn_interrupt: u64,
    #[serde(default)]
    pub config_requirements_read: u64,
    /// Approval responses actually written to the provider, by decision.
    #[serde(default)]
    pub approval_accept: u64,
    #[serde(default)]
    pub approval_decline: u64,
    #[serde(default)]
    pub approval_cancel: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeStatus {
    pub adapter_id: AgentAdapterId,
    pub runtime_kind: AgentRuntimeKind,
    pub desired_state: DesiredState,
    pub revision: u64,
    pub actual_state: ActualState,
    pub runtime_instance_id: Option<super::identities::AgentRuntimeInstanceId>,
    pub runtime_version: Option<String>,
    pub compatibility: super::agent_adapter::CompatibilityState,
    pub model_count: u16,
    pub diagnostic: Option<RuntimeDiagnostic>,
    pub outbound_methods: RuntimeMethods,
    #[serde(default)]
    pub runtime_safety_profile: Option<RuntimeSafetyProfile>,
    /// The workspace a ready local-tools generation is isolated for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
}
impl RuntimeStatus {
    pub fn validate(&self) -> ControlResult<()> {
        RuntimeTarget {
            adapter_id: self.adapter_id.clone(),
            runtime_kind: self.runtime_kind,
        }
        .supported()?;
        if self.model_count > 50
            || self
                .runtime_version
                .as_ref()
                .is_some_and(|s| s.len() > 128 || s.chars().any(char::is_control))
            || self.outbound_methods.initialize > 3
            || self.outbound_methods.initialized > 3
            || self.outbound_methods.model_list > 3
        {
            return Err(ControlError::InvalidRequest);
        }
        if self.actual_state != ActualState::Ready && self.model_count != 0 {
            return Err(ControlError::InvalidRequest);
        }
        if self.workspace_root.is_some()
            != (self.runtime_safety_profile == Some(RuntimeSafetyProfile::CodexLocalToolsV1))
        {
            return Err(ControlError::InvalidRequest);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCursor {
    pub runtime_instance_id: super::identities::AgentRuntimeInstanceId,
    pub index: u16,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeModels {
    pub adapter_id: AgentAdapterId,
    pub runtime_kind: AgentRuntimeKind,
    pub cursor: Option<ModelCursor>,
    pub limit: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSafetyProfile {
    CodexInferenceV1,
    /// Isolated generation offering exactly one-shot command execution and patch
    /// application, each mediated by the authority.
    CodexLocalToolsV1,
}
