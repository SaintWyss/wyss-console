//! Wyss Control Protocol v1. No runtime execution operations exist.
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
pub const METHODS: [&str; 7] = [
    "authority/hello",
    "authority/status",
    "managedSession/prepare",
    "managedSession/get",
    "managedSession/list",
    "managedSession/close",
    "audit/read",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlError {
    ProtocolVersionUnsupported,
    HandshakeRequired,
    InvalidRequest,
    UnsupportedMethod,
    FrameTooLarge,
    TruncatedFrame,
    Timeout,
    PeerRejected,
    UnsafePath,
    AlreadyRunning,
    StorageUnavailable,
    StorageIncompatible,
    SessionNotFound,
    InvalidTransition,
    StaleRevision,
    IntentConflict,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", deny_unknown_fields)]
pub enum Operation {
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
    #[serde(rename = "audit/read")]
    AuditRead(AuditRead),
}
impl Operation {
    pub fn validate(&self) -> ControlResult<()> {
        match self {
            Self::Hello(h) if h.protocol_version != PROTOCOL_VERSION => {
                Err(ControlError::ProtocolVersionUnsupported)
            }
            Self::List(p) if p.limit == 0 || p.limit > MAX_PAGE => {
                Err(ControlError::InvalidRequest)
            }
            Self::AuditRead(p) if p.limit == 0 || p.limit > MAX_PAGE => {
                Err(ControlError::InvalidRequest)
            }
            Self::Close(p) if !(1..=2).contains(&p.expected_revision) => {
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
    Hello(AuthorityStatus),
    Status(AuthorityStatus),
    Session(ManagedSessionRecord),
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
                || s.storage_schema_version != 1
                || s.service_version.len() > 128
            {
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
