//! Strict protocol-2 request codec. Parsing is not execution authorization.
use crate::contracts::{
    WireUuid,
    control::{self as v1, ControlError as E, ControlResult},
    identities::{AgentRuntimeInstanceId, ManagedSessionId},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeFence {
    pub managed_session_id: ManagedSessionId,
    pub generation: u64,
    pub runtime_instance_id: Option<AgentRuntimeInstanceId>,
}
impl RuntimeFence {
    pub fn validate(&self, allow_absent: bool) -> ControlResult<()> {
        canonical(self.managed_session_id.as_str())?;
        if self.generation > i64::MAX as u64
            || (!allow_absent && (self.generation == 0 || self.runtime_instance_id.is_none()))
            || (self.generation == 0 && self.runtime_instance_id.is_some())
        {
            return Err(E::InvalidRequest);
        }
        if let Some(id) = &self.runtime_instance_id {
            canonical(id.as_str())?;
            if id.as_str() == self.managed_session_id.as_str() {
                return Err(E::InvalidRequest);
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeRef {
    pub schema: u8,
    pub authority_instance_id: WireUuid,
    pub managed_session_id: ManagedSessionId,
    pub runtime_instance_id: AgentRuntimeInstanceId,
    pub generation: u64,
}
impl RuntimeRef {
    pub fn validate(&self) -> ControlResult<()> {
        if self.schema != 2 {
            return Err(E::ProtocolVersionUnsupported);
        }
        canonical(self.authority_instance_id.as_str())?;
        self.fence().validate(false)?;
        if self.authority_instance_id.as_str() == self.managed_session_id.as_str()
            || self.authority_instance_id.as_str() == self.runtime_instance_id.as_str()
        {
            return Err(E::InvalidRequest);
        }
        Ok(())
    }
    pub fn fence(&self) -> RuntimeFence {
        RuntimeFence {
            managed_session_id: self.managed_session_id.clone(),
            generation: self.generation,
            runtime_instance_id: Some(self.runtime_instance_id.clone()),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeIdentity {
    pub schema: u8,
    pub authority_instance_id: WireUuid,
    pub managed_session_id: ManagedSessionId,
    pub runtime_instance_id: AgentRuntimeInstanceId,
    pub generation: u64,
    pub observed_version: String,
}
impl RuntimeIdentity {
    pub fn reference(&self) -> RuntimeRef {
        RuntimeRef {
            schema: self.schema,
            authority_instance_id: self.authority_instance_id.clone(),
            managed_session_id: self.managed_session_id.clone(),
            runtime_instance_id: self.runtime_instance_id.clone(),
            generation: self.generation,
        }
    }
    pub fn validate(&self) -> ControlResult<()> {
        self.reference().validate()?;
        if self.observed_version.is_empty()
            || self.observed_version.len() > 128
            || !self
                .observed_version
                .bytes()
                .all(|b| b.is_ascii_digit() || b == b'.')
            || self.observed_version.split('.').any(str::is_empty)
        {
            return Err(E::InvalidRequest);
        }
        Ok(())
    }
}

/// Additional targeting is retained as typed data, never discarded on decode.
#[derive(Debug, Clone)]
pub enum Target {
    None,
    Runtime {
        session: ManagedSessionId,
        generation: Option<u64>,
        instance: Option<AgentRuntimeInstanceId>,
    },
    Fenced(RuntimeFence),
}
#[derive(Debug, Clone)]
pub struct Request {
    request_id: WireUuid,
    authority: Option<WireUuid>,
    operation: v1::Operation,
    target: Target,
}
impl Request {
    pub fn parse(bytes: &[u8]) -> ControlResult<Self> {
        let mut v = v1::unique_json(bytes)?;
        if v.get("schema_version").and_then(Value::as_u64) != Some(2) {
            return Err(E::ProtocolVersionUnsupported);
        }
        let hello = v["method"] == "authority/hello";
        exact(
            &v,
            if hello {
                &["schema_version", "request_id", "method", "params"]
            } else {
                &[
                    "schema_version",
                    "request_id",
                    "method",
                    "params",
                    "authority_instance_id",
                ]
            },
        )?;
        let request_id = decode(v["request_id"].clone())?;
        let authority: Option<WireUuid> = if hello {
            None
        } else {
            Some(decode(v["authority_instance_id"].clone())?)
        };
        canonical_ids(&v)?;
        let method = v["method"].as_str().ok_or(E::InvalidRequest)?.to_owned();
        if !v1::METHODS.contains(&method.as_str()) {
            return Err(E::UnsupportedMethod);
        }
        let p = v["params"].as_object_mut().ok_or(E::InvalidRequest)?;
        let target = if ROUTED.contains(&method.as_str()) {
            let raw_fence = p.remove("runtime_fence").ok_or(E::InvalidRequest)?;
            exact(
                &raw_fence,
                &["managed_session_id", "generation", "runtime_instance_id"],
            )?;
            let fence: RuntimeFence = decode(raw_fence)?;
            let allow_absent = matches!(
                method.as_str(),
                "managedSession/close" | "managedSession/runtime/abandon"
            );
            fence.validate(allow_absent)?;
            if ["managed_session_id", "session_id"].iter().any(|key| {
                p.get(*key)
                    .is_some_and(|s| s.as_str() != Some(fence.managed_session_id.as_str()))
            }) {
                return Err(E::InvalidRequest);
            }
            different_authority(authority.as_ref(), &fence)?;
            Target::Fenced(fence)
        } else if method.starts_with("runtime/") {
            let session: ManagedSessionId =
                decode(p.remove("managed_session_id").ok_or(E::InvalidRequest)?)?;
            let generation = if method == "runtime/status" {
                None
            } else {
                Some(u63(p
                    .remove("expected_generation")
                    .ok_or(E::InvalidRequest)?)?)
            };
            if matches!(method.as_str(), "runtime/start" | "runtime/stop") {
                u63(p
                    .get("expected_revision")
                    .cloned()
                    .ok_or(E::InvalidRequest)?)?;
            }
            let instance = if matches!(method.as_str(), "runtime/stop" | "runtime/model/list") {
                let val = p
                    .remove("expected_runtime_instance_id")
                    .ok_or(E::InvalidRequest)?;
                if val.is_null() {
                    None
                } else {
                    Some(decode(val)?)
                }
            } else {
                None
            };
            let fence = RuntimeFence {
                managed_session_id: session.clone(),
                generation: generation.unwrap_or(0),
                runtime_instance_id: instance.clone(),
            };
            if method == "runtime/model/list" {
                fence.validate(false)?;
            } else if method == "runtime/stop" {
                fence.validate(true)?;
            }
            different_authority(authority.as_ref(), &fence)?;
            Target::Runtime {
                session,
                generation,
                instance,
            }
        } else {
            Target::None
        };
        let operation: v1::Operation = decode(json!({"method":method,"params":v["params"]}))?;
        if let v1::Operation::Hello(h) = &operation {
            if h.protocol_version != 2 {
                return Err(E::ProtocolVersionUnsupported);
            }
        } else {
            operation.validate()?;
        }
        if let (v1::Operation::RuntimeModels(models), Target::Runtime { instance, .. }) =
            (&operation, &target)
            && models
                .cursor
                .as_ref()
                .is_some_and(|c| Some(&c.runtime_instance_id) != instance.as_ref() || c.index > 50)
        {
            return Err(E::InvalidRequest);
        }
        Ok(Self {
            request_id,
            authority,
            operation,
            target,
        })
    }
    pub fn request_id(&self) -> &WireUuid {
        &self.request_id
    }
    pub fn authority(&self) -> Option<&WireUuid> {
        self.authority.as_ref()
    }
    pub fn operation(&self) -> &v1::Operation {
        &self.operation
    }
    pub fn target(&self) -> &Target {
        &self.target
    }
    /// Must be checked at handshake/dispatch, not inferred from a selected GUI session.
    pub fn correlate(&self, current: Option<&WireUuid>) -> ControlResult<()> {
        match (&self.operation, current, &self.authority) {
            (v1::Operation::Hello(_), None, None) => Ok(()),
            (v1::Operation::Hello(_), _, _) => Err(E::InvalidRequest),
            (_, None, _) => Err(E::HandshakeRequired),
            (_, Some(a), Some(b)) if a == b => Ok(()),
            _ => Err(E::CorrelationError),
        }
    }
    pub fn encode(&self) -> ControlResult<Vec<u8>> {
        let mut v = serde_json::to_value(&self.operation).map_err(|_| E::InvalidRequest)?;
        v["schema_version"] = json!(2);
        v["request_id"] = json!(self.request_id);
        if let Some(a) = &self.authority {
            v["authority_instance_id"] = json!(a);
        }
        match &self.target {
            Target::None => (),
            Target::Fenced(f) => v["params"]["runtime_fence"] = json!(f),
            Target::Runtime {
                session,
                generation,
                instance,
            } => {
                v["params"]["managed_session_id"] = json!(session);
                if let Some(g) = generation {
                    v["params"]["expected_generation"] = json!(g);
                }
                if matches!(
                    self.operation,
                    v1::Operation::RuntimeStop(_) | v1::Operation::RuntimeModels(_)
                ) {
                    v["params"]["expected_runtime_instance_id"] = json!(instance);
                }
            }
        }
        let bytes = encode(&v)?;
        Self::parse(&bytes)?;
        Ok(bytes)
    }
    /// Canonical intent body, including targeting, excludes transport request/authority IDs.
    /// Contains the original typed turn prompt; callers must fingerprint it before persistence.
    pub fn canonical_intent(&self) -> ControlResult<Vec<u8>> {
        let mut v = v1::unique_json(&self.encode()?)?;
        let o = v.as_object_mut().ok_or(E::InvalidRequest)?;
        for key in ["schema_version", "request_id", "authority_instance_id"] {
            o.remove(key);
        }
        encode(&v)
    }
}
const ROUTED: [&str; 9] = [
    "managedSession/runtime/create",
    "managedSession/runtime/resume",
    "managedSession/runtime/abandon",
    "managedSession/turn/start",
    "managedSession/turn/cancel",
    "managedSession/close",
    "executionLease/grant",
    "executionLease/renew",
    "managedTurn/approval/respond",
];
fn different_authority(a: Option<&WireUuid>, f: &RuntimeFence) -> ControlResult<()> {
    if a.is_some_and(|a| {
        a.as_str() == f.managed_session_id.as_str()
            || f.runtime_instance_id
                .as_ref()
                .is_some_and(|i| i.as_str() == a.as_str())
    }) {
        Err(E::InvalidRequest)
    } else {
        Ok(())
    }
}
pub(super) fn canonical(s: &str) -> ControlResult<()> {
    let u = uuid::Uuid::parse_str(s).map_err(|_| E::InvalidRequest)?;
    if u.to_string() != s {
        Err(E::InvalidRequest)
    } else {
        Ok(())
    }
}
fn canonical_ids(v: &Value) -> ControlResult<()> {
    match v {
        Value::Object(m) => {
            for (k, v) in m {
                if matches!(
                    k.as_str(),
                    "request_id"
                        | "authority_instance_id"
                        | "client_instance_id"
                        | "managed_session_id"
                        | "session_id"
                        | "runtime_instance_id"
                        | "expected_runtime_instance_id"
                        | "intent_id"
                        | "lease_id"
                        | "managed_turn_id"
                        | "managed_approval_id"
                        | "operation_id"
                        | "stream_id"
                ) && !v.is_null()
                {
                    canonical(v.as_str().ok_or(E::InvalidRequest)?)?;
                }
                canonical_ids(v)?;
            }
        }
        Value::Array(v) => {
            for v in v {
                canonical_ids(v)?;
            }
        }
        _ => (),
    }
    Ok(())
}
fn u63(v: Value) -> ControlResult<u64> {
    v.as_u64()
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or(E::InvalidRequest)
}
pub(super) fn decode<T: serde::de::DeserializeOwned>(v: Value) -> ControlResult<T> {
    serde_json::from_value(v).map_err(|_| E::InvalidRequest)
}
pub(super) fn exact(v: &Value, keys: &[&str]) -> ControlResult<()> {
    let m = v.as_object().ok_or(E::InvalidRequest)?;
    if m.len() != keys.len() || keys.iter().any(|k| !m.contains_key(*k)) {
        Err(E::InvalidRequest)
    } else {
        Ok(())
    }
}
pub(super) fn encode(v: &Value) -> ControlResult<Vec<u8>> {
    let b = serde_json::to_vec(v).map_err(|_| E::InvalidRequest)?;
    if b.len() > v1::MAX_FRAME {
        Err(E::FrameTooLarge)
    } else {
        Ok(b)
    }
}

/// Typed result targeting, separate from immutable historical records and transient attestation.
#[derive(Debug, Clone)]
pub enum Projection {
    None,
    Runtime {
        session: ManagedSessionId,
        generation: u64,
        identity: Option<RuntimeIdentity>,
        budget_failure: Option<String>,
    },
    History {
        reference: Option<RuntimeRef>,
        attestation: Option<RuntimeIdentity>,
    },
    Attachment(Option<RuntimeIdentity>),
}
#[derive(Debug, Clone)]
pub struct Response {
    request_id: WireUuid,
    authority: WireUuid,
    outcome: ControlResult<v1::ResultValue>,
    projection: Projection,
    budget_exceeded: bool,
}
impl Response {
    pub fn parse(bytes: &[u8]) -> ControlResult<Self> {
        let mut v = v1::unique_json(bytes)?;
        if v.get("schema_version").and_then(Value::as_u64) != Some(2) {
            return Err(E::ProtocolVersionUnsupported);
        }
        let result = v.get("result").is_some();
        exact(
            &v,
            if result {
                &[
                    "schema_version",
                    "request_id",
                    "authority_instance_id",
                    "result",
                ]
            } else {
                &[
                    "schema_version",
                    "request_id",
                    "authority_instance_id",
                    "error",
                ]
            },
        )?;
        canonical_ids(&v)?;
        let authority: WireUuid = decode(v["authority_instance_id"].clone())?;
        let mut projection = Projection::None;
        let mut budget_exceeded = false;
        if result {
            exact(&v["result"], &["kind", "value"])?;
            let kind = v["result"]["kind"]
                .as_str()
                .ok_or(E::InvalidRequest)?
                .to_owned();
            let data = &mut v["result"]["value"];
            match kind.as_str() {
                "hello" | "status" => {
                    if data["protocol_version"].as_u64() != Some(2) {
                        return Err(E::ProtocolVersionUnsupported);
                    }
                    if data["storage_schema_version"].as_u64() != Some(8) {
                        return Err(E::StorageIncompatible);
                    }
                    if data["authority_instance_id"] != json!(authority) {
                        return Err(E::CorrelationError);
                    }
                    data["protocol_version"] = json!(1);
                    data["storage_schema_version"] = json!(7);
                }
                "runtime_control" | "runtime_status" => {
                    let o = data.as_object_mut().ok_or(E::InvalidRequest)?;
                    let session: ManagedSessionId =
                        decode(o.remove("managed_session_id").ok_or(E::InvalidRequest)?)?;
                    if session.as_str() == authority.as_str() {
                        return Err(E::InvalidRequest);
                    }
                    let generation = u63(o.remove("generation").ok_or(E::InvalidRequest)?)?;
                    let identity = if kind == "runtime_status" {
                        decode(o.remove("runtime_identity").ok_or(E::InvalidRequest)?)?
                    } else {
                        None
                    };
                    let budget_failure: Option<String> = if kind == "runtime_status" {
                        decode(o.remove("budget_failure").ok_or(E::InvalidRequest)?)?
                    } else {
                        None
                    };
                    if let Some(key) = &budget_failure {
                        let resources: Value = serde_json::from_str(include_str!(
                            "../../../docs/contracts/f8/resources.json"
                        ))
                        .map_err(|_| E::InternalError)?;
                        if key == "contract_version"
                            || resources.get(key).and_then(Value::as_u64).is_none()
                        {
                            return Err(E::InvalidRequest);
                        }
                    }
                    if kind == "runtime_status" {
                        budget_exceeded = o.get("diagnostic") == Some(&json!("budget_exceeded"));
                        if budget_exceeded != budget_failure.is_some() {
                            return Err(E::InvalidRequest);
                        }
                        if budget_exceeded {
                            o.insert("diagnostic".into(), Value::Null);
                        }
                    }
                    projection = Projection::Runtime {
                        session,
                        generation,
                        identity,
                        budget_failure,
                    };
                }
                "runtime_models" => {
                    let o = data.as_object_mut().ok_or(E::InvalidRequest)?;
                    let identity: RuntimeIdentity =
                        decode(o.remove("runtime_identity").ok_or(E::InvalidRequest)?)?;
                    identity.validate()?;
                    if identity.authority_instance_id != authority
                        || o.get("runtime_instance_id")
                            != Some(&json!(identity.runtime_instance_id))
                    {
                        return Err(E::CorrelationError);
                    }
                    projection = Projection::Runtime {
                        session: identity.managed_session_id.clone(),
                        generation: identity.generation,
                        identity: Some(identity),
                        budget_failure: None,
                    };
                }
                "binding_status" => {
                    let o = data.as_object_mut().ok_or(E::InvalidRequest)?;
                    projection = Projection::Attachment(decode(
                        o.remove("runtime_identity").ok_or(E::InvalidRequest)?,
                    )?);
                }
                "turn_status" | "approval" | "pending_approval" if !data.is_null() => {
                    let o = data.as_object_mut().ok_or(E::InvalidRequest)?;
                    let reference: Option<RuntimeRef> =
                        decode(o.remove("runtime_ref").ok_or(E::InvalidRequest)?)?;
                    let attestation: Option<RuntimeIdentity> =
                        decode(o.remove("attestation").ok_or(E::InvalidRequest)?)?;
                    if let Some(r) = &reference {
                        r.validate()?;
                    }
                    if let Some(a) = &attestation {
                        a.validate()?;
                        if reference.as_ref() != Some(&a.reference())
                            || a.authority_instance_id != authority
                        {
                            return Err(E::CorrelationError);
                        }
                    }
                    projection = Projection::History {
                        reference,
                        attestation,
                    };
                }
                // Event payloads are validated against their declared vocabulary in validate_projection.
                _ => (),
            }
        }
        v["schema_version"] = json!(1);
        let mut base = v1::Response::parse(&encode(&v)?)?;
        if let Ok(v1::ResultValue::Hello(s) | v1::ResultValue::Status(s)) = &mut base.outcome {
            s.protocol_version = 2;
            s.storage_schema_version = 8;
        }
        let response = Self {
            request_id: base.request_id,
            authority: base.authority_instance_id,
            outcome: base.outcome,
            projection,
            budget_exceeded,
        };
        response.validate_projection()?;
        Ok(response)
    }
    fn validate_projection(&self) -> ControlResult<()> {
        match (&self.outcome, &self.projection) {
            (Ok(v1::ResultValue::Audit { events, .. }), _) => {
                for event in events {
                    let value = serde_json::to_value(event).map_err(|_| E::InvalidRequest)?;
                    super::audit::validate_event(&value)?;
                }
            }
            (
                Ok(v1::ResultValue::RuntimeModels { next, .. }),
                Projection::Runtime {
                    identity: Some(identity),
                    ..
                },
            ) => {
                if next.as_ref().is_some_and(|c| {
                    c.runtime_instance_id != identity.runtime_instance_id || c.index > 50
                }) {
                    return Err(E::CorrelationError);
                }
            }
            (
                Ok(v1::ResultValue::RuntimeControl(_)),
                Projection::Runtime { identity: None, .. },
            ) => (),
            (
                Ok(v1::ResultValue::RuntimeStatus(s)),
                Projection::Runtime {
                    session,
                    generation,
                    identity,
                    ..
                },
            ) => {
                if s.actual_state == v1::ActualState::Ready && identity.is_none() {
                    return Err(E::InvalidRequest);
                }
                if let Some(i) = identity {
                    i.validate()?;
                    if i.authority_instance_id != self.authority
                        || i.managed_session_id != *session
                        || i.generation != *generation
                        || s.runtime_instance_id.as_ref() != Some(&i.runtime_instance_id)
                        || s.runtime_version.as_deref() != Some(i.observed_version.as_str())
                    {
                        return Err(E::CorrelationError);
                    }
                }
            }
            (Ok(v1::ResultValue::BindingStatus(s)), Projection::Attachment(identity)) => {
                if identity.is_some() != (s.state == v1::BindingState::BoundAttached) {
                    return Err(E::InvalidRequest);
                }
                if let Some(i) = identity {
                    i.validate()?;
                    if i.authority_instance_id != self.authority
                        || i.managed_session_id != s.managed_session_id
                        || s.runtime_instance_id.as_ref() != Some(&i.runtime_instance_id)
                    {
                        return Err(E::CorrelationError);
                    }
                }
            }
            (
                Ok(r),
                Projection::History {
                    reference: Some(reference),
                    ..
                },
            ) => {
                let (session, instance) = match r {
                    v1::ResultValue::TurnStatus(s) => {
                        (&s.turn.managed_session_id, &s.turn.runtime_instance_id)
                    }
                    v1::ResultValue::Approval(s) => (
                        &s.approval.managed_session_id,
                        &s.approval.runtime_instance_id,
                    ),
                    v1::ResultValue::PendingApproval(Some(s)) => (
                        &s.approval.managed_session_id,
                        &s.approval.runtime_instance_id,
                    ),
                    _ => return Err(E::InvalidRequest),
                };
                if session != &reference.managed_session_id
                    || instance != &reference.runtime_instance_id
                {
                    return Err(E::CorrelationError);
                }
            }
            _ => (),
        }
        Ok(())
    }
    pub fn outcome(&self) -> &ControlResult<v1::ResultValue> {
        &self.outcome
    }
    pub fn projection(&self) -> &Projection {
        &self.projection
    }
    pub fn correlate(&self, request: &Request, authority: &WireUuid) -> ControlResult<()> {
        self.correlate_with_context(request, authority, None)
    }
    /// A binding create canonicalizes its workspace before dispatch. Supply that independently
    /// accepted canonical path; never resolve it here, especially when checking a replay receipt.
    pub fn correlate_binding_create(
        &self,
        request: &Request,
        authority: &WireUuid,
        canonical_workspace: &str,
    ) -> ControlResult<()> {
        if !matches!(request.operation(), v1::Operation::BindingCreate(_)) {
            return Err(E::InvalidRequest);
        }
        v1::validate_workspace_text(canonical_workspace)?;
        self.correlate_with_context(request, authority, Some(canonical_workspace))
    }
    fn correlate_with_context(
        &self,
        request: &Request,
        authority: &WireUuid,
        canonical_workspace: Option<&str>,
    ) -> ControlResult<()> {
        if self.request_id != *request.request_id()
            || self.authority != *authority
            || request.authority().is_some_and(|a| a != authority)
        {
            return Err(E::CorrelationError);
        }
        if let Ok(r) = &self.outcome {
            let value = serde_json::to_value(r).map_err(|_| E::InvalidRequest)?;
            let expected = match request.operation() {
                v1::Operation::Hello(_) => "hello",
                v1::Operation::Status(_) => "status",
                v1::Operation::RuntimeStatus(_) => "runtime_status",
                v1::Operation::RuntimeModels(_) => "runtime_models",
                v1::Operation::RuntimeStart(_) | v1::Operation::RuntimeStop(_) => "runtime_control",
                v1::Operation::Prepare(_) | v1::Operation::Get(_) | v1::Operation::Close(_) => {
                    "session"
                }
                v1::Operation::List(_) => "sessions",
                v1::Operation::BindingCreate(_) | v1::Operation::BindingResume(_) => "binding",
                v1::Operation::BindingStatus(_) => "binding_status",
                v1::Operation::BindingAbandon(_) => "binding_abandoned",
                v1::Operation::LeaseGrant(_)
                | v1::Operation::LeaseGet(_)
                | v1::Operation::LeaseRenew(_)
                | v1::Operation::LeaseRevoke(_) => "lease",
                v1::Operation::LeaseActive(_) => "active_lease",
                v1::Operation::GateStatus(_) => "gate_status",
                v1::Operation::TurnStart(_) | v1::Operation::TurnCancel(_) => "turn",
                v1::Operation::TurnStatus(_) => "turn_status",
                v1::Operation::TurnEvents(_) => "turn_events",
                v1::Operation::ApprovalPending(_) => "pending_approval",
                v1::Operation::ApprovalStatus(_) => "approval",
                v1::Operation::ApprovalRespond(_) => "approval_receipt",
                v1::Operation::AuditRead(_) | v1::Operation::AuditWatch(_) => "audit",
            };
            if value["kind"] != expected {
                return Err(E::CorrelationError);
            }
            correlate_record(request, r, canonical_workspace)?;
            if let (
                v1::ResultValue::RuntimeControl(record),
                Projection::Runtime { generation, .. },
            ) = (r, &self.projection)
            {
                let (mutation, start) = match request.operation() {
                    v1::Operation::RuntimeStart(p) => (p, true),
                    v1::Operation::RuntimeStop(p) => (p, false),
                    _ => return Err(E::CorrelationError),
                };
                let Target::Runtime {
                    generation: Some(expected_generation),
                    ..
                } = request.target()
                else {
                    return Err(E::CorrelationError);
                };
                let expected_revision = mutation.expected_revision.ok_or(E::CorrelationError)?;
                let valid_generation = if start {
                    *generation == *expected_generation
                        || expected_generation.checked_add(1) == Some(*generation)
                } else {
                    *generation == *expected_generation
                };
                if !valid_generation
                    || record.revision
                        != expected_revision
                            .checked_add(1)
                            .ok_or(E::CorrelationError)?
                    || record.desired_state
                        != if start {
                            v1::DesiredState::Running
                        } else {
                            v1::DesiredState::Stopped
                        }
                    || (start && *expected_generation == 0 && *generation != 1)
                {
                    return Err(E::CorrelationError);
                }
            }
            if let (
                Target::Runtime {
                    session,
                    generation,
                    instance,
                },
                Projection::Runtime {
                    session: actual,
                    generation: g,
                    identity,
                    ..
                },
            ) = (request.target(), &self.projection)
                && (session != actual
                    || (matches!(request.operation(), v1::Operation::RuntimeModels(_))
                        && (generation != &Some(*g)
                            || identity.as_ref().map(|i| &i.runtime_instance_id)
                                != instance.as_ref())))
            {
                return Err(E::CorrelationError);
            }
        }
        Ok(())
    }
    pub fn error(request_id: WireUuid, authority: WireUuid, error: E) -> ControlResult<Self> {
        Self::parse(&encode(
            &json!({"schema_version":2,"request_id":request_id,"authority_instance_id":authority,"error":{"code":error}}),
        )?)
    }
    pub fn encode(&self) -> ControlResult<Vec<u8>> {
        let mut v = json!({"schema_version":2,"request_id":self.request_id,"authority_instance_id":self.authority});
        match &self.outcome {
            Err(e) => v["error"] = json!({"code":e}),
            Ok(r) => {
                v["result"] = serde_json::to_value(r).map_err(|_| E::InvalidRequest)?;
                let kind = v["result"]["kind"]
                    .as_str()
                    .ok_or(E::InvalidRequest)?
                    .to_owned();
                let d = &mut v["result"]["value"];
                if matches!(kind.as_str(), "hello" | "status") {
                    d["protocol_version"] = json!(2);
                    d["storage_schema_version"] = json!(8);
                }
                match &self.projection {
                    Projection::None => (),
                    Projection::Attachment(i) => d["runtime_identity"] = json!(i),
                    Projection::History {
                        reference,
                        attestation,
                    } => {
                        d["runtime_ref"] = json!(reference);
                        d["attestation"] = json!(attestation);
                    }
                    Projection::Runtime {
                        session,
                        generation,
                        identity,
                        budget_failure,
                    } => {
                        if kind != "runtime_models" {
                            d["managed_session_id"] = json!(session);
                            d["generation"] = json!(generation);
                        }
                        if kind != "runtime_control" {
                            d["runtime_identity"] = json!(identity);
                        }
                        if kind == "runtime_status" {
                            d["budget_failure"] = json!(budget_failure);
                            if self.budget_exceeded {
                                d["diagnostic"] = json!("budget_exceeded");
                            }
                        }
                    }
                }
            }
        }
        let b = encode(&v)?;
        Self::parse(&b)?;
        Ok(b)
    }
}

fn correlate_record(
    request: &Request,
    result: &v1::ResultValue,
    canonical_workspace: Option<&str>,
) -> ControlResult<()> {
    use v1::{Operation as O, ResultValue as R};
    let fence_session = |session: &ManagedSessionId| match request.target() {
        Target::Fenced(f) => &f.managed_session_id == session,
        _ => true,
    };
    let fence_turn = |turn: &v1::ManagedTurnRecord| match request.target() {
        Target::Fenced(f) => {
            f.managed_session_id == turn.managed_session_id
                && f.runtime_instance_id.as_ref() == Some(&turn.runtime_instance_id)
        }
        _ => false,
    };
    let next_revision = |expected: u64, actual: u64| expected.checked_add(1) == Some(actual);
    let matches = match (request.operation(), result) {
        (O::Prepare(p), R::Session(s)) => {
            s.adapter_id == p.adapter_id
                && s.runtime_kind == p.runtime_kind
                && s.control_state == v1::ControlState::Prepared
                && s.revision == 1
        }
        (O::Get(p), R::Session(s)) => p.managed_session_id == s.managed_session_id,
        (O::Close(p), R::Session(s)) => {
            p.managed_session_id == s.managed_session_id
                && s.control_state == v1::ControlState::Closed
                && (next_revision(p.expected_revision, s.revision)
                    || p.expected_revision == 2 && s.revision == 2)
        }
        (O::List(p), R::Sessions { sessions, next }) => {
            let increasing = sessions
                .windows(2)
                .all(|w| w[0].created_sequence < w[1].created_sequence);
            let ids: std::collections::BTreeSet<_> = sessions
                .iter()
                .map(|s| s.managed_session_id.as_str())
                .collect();
            sessions.len() <= usize::from(p.limit)
                && ids.len() == sessions.len()
                && increasing
                && sessions
                    .iter()
                    .all(|s| p.after.is_none_or(|a| s.created_sequence > a))
                && next.is_none_or(|n| {
                    sessions.len() == usize::from(p.limit)
                        && sessions.last().is_some_and(|s| s.created_sequence == n)
                })
        }
        (O::BindingCreate(p), R::Binding(b)) => {
            p.managed_session_id == b.managed_session_id
                && canonical_workspace.ok_or(E::Unsupported)? == b.workspace_root
        }
        (O::BindingResume(p), R::Binding(b)) => p.managed_session_id == b.managed_session_id,
        (O::BindingStatus(p), R::BindingStatus(b)) => p.managed_session_id == b.managed_session_id,
        (O::BindingAbandon(p), R::BindingAbandoned(b)) => p.operation_id == b.operation_id,
        (O::LeaseGrant(p), R::Lease(l)) => {
            let mut scopes = p.scopes.clone();
            scopes.sort();
            p.session_id == l.session_id
                && scopes == l.scopes
                && l.state == v1::LeaseState::Active
                && l.revision == 1
                && (v1::timestamp(&l.expires_at)? - v1::timestamp(&l.issued_at)?)
                    == time::Duration::seconds(i64::from(p.ttl_seconds))
        }
        (O::LeaseGet(p), R::Lease(l)) => p.lease_id == l.lease_id,
        (O::LeaseActive(p), R::ActiveLease(l)) => l
            .as_ref()
            .is_none_or(|l| p.session_id == l.session_id && l.state == v1::LeaseState::Active),
        (O::LeaseRenew(p), R::Lease(l)) => {
            p.lease_id == l.lease_id
                && fence_session(&l.session_id)
                && l.state == v1::LeaseState::Active
                && next_revision(p.expected_revision, l.revision)
                && (v1::timestamp(&l.expires_at)? - v1::timestamp(&l.updated_at)?)
                    == time::Duration::seconds(i64::from(p.ttl_seconds))
        }
        (O::LeaseRevoke(p), R::Lease(l)) => {
            p.lease_id == l.lease_id
                && l.state == v1::LeaseState::Revoked
                && l.revocation_reason == Some(v1::RevocationReason::UserRequest)
                && next_revision(p.expected_revision, l.revision)
        }
        (O::GateStatus(p), R::GateStatus(g)) => {
            p.session_id == g.session_id && p.action == g.action
        }
        (O::TurnStart(p), R::Turn(t)) => {
            p.managed_session_id == t.managed_session_id && fence_turn(t)
        }
        (O::TurnCancel(p), R::Turn(t)) => p.managed_turn_id == t.managed_turn_id && fence_turn(t),
        (O::TurnStatus(p), R::TurnStatus(t)) => p.managed_turn_id == t.turn.managed_turn_id,
        (O::TurnEvents(p), R::TurnEvents(page)) => {
            page.events.len() <= usize::from(p.limit)
                && page.next_sequence == page.events.last().map(|e| e.sequence)
                && page.gap
                    == page
                        .events
                        .first()
                        .is_some_and(|e| e.kind == v1::StreamKind::StreamGap)
                && page.events.iter().enumerate().all(|(index, e)| {
                    let after = p.after_sequence.map_or(0, |s| s.get());
                    if e.kind == v1::StreamKind::StreamGap {
                        let expected = page
                            .earliest_sequence
                            .map_or(after.saturating_add(1).max(1), |s| {
                                s.get().saturating_sub(1).max(1)
                            });
                        index == 0
                            && page.gap
                            && e.sequence.get() == expected
                            && e.text.is_none()
                            && e.managed_approval_id.is_none()
                    } else {
                        e.sequence.get() > after
                    }
                })
                && page
                    .events
                    .iter()
                    .all(|e| e.managed_turn_id == p.managed_turn_id)
                && page.events.windows(2).all(|w| {
                    w[0].sequence < w[1].sequence
                        && w[0].managed_session_id == w[1].managed_session_id
                })
        }
        (O::ApprovalPending(p), R::PendingApproval(a)) => a
            .as_ref()
            .is_none_or(|a| p.managed_turn_id == a.approval.managed_turn_id),
        (O::ApprovalStatus(p), R::Approval(a)) => {
            p.managed_approval_id == a.approval.managed_approval_id
        }
        (O::ApprovalRespond(p), R::ApprovalReceipt(a)) => {
            p.managed_approval_id == a.managed_approval_id
                && p.decision == a.decision
                && next_revision(p.expected_revision, a.revision)
        }
        (O::AuditRead(p), R::Audit { events, next }) => {
            super::audit::check_page(&p.stream_id, p.after_sequence, p.limit, events, *next).is_ok()
        }
        (O::AuditWatch(p), R::Audit { events, next }) => {
            super::audit::check_page(&p.stream_id, p.after_sequence, p.limit, events, *next).is_ok()
        }
        // Runtime/authority variants are checked by the dedicated correlation above.
        (O::Hello(_), R::Hello(_))
        | (O::Status(_), R::Status(_))
        | (O::RuntimeStatus(_), R::RuntimeStatus(_))
        | (O::RuntimeModels(_), R::RuntimeModels { .. })
        | (O::RuntimeStart(_) | O::RuntimeStop(_), R::RuntimeControl(_)) => true,
        _ => false,
    };
    if matches {
        Ok(())
    } else {
        Err(E::CorrelationError)
    }
}
