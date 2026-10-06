//! Descriptive wire contracts, independent of Tauri, execution and authorization.
//! Only validated root DTOs are constructed at a trust boundary. Evidence is a
//! producer's declaration, never an authorization or an attestation of safety.
use super::{
    UtcTimestamp, WireUuid,
    identities::{AgentRuntimeInstanceId, ManagedSessionId, TerminalId},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, fmt};
#[cfg(test)]
mod tests;
mod validation;

macro_rules! wire_enum {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }
    };
}
wire_enum!(AgentRuntimeKind {
    Codex,
    ClaudeCode,
    Opencode,
    GenericCli
});
wire_enum!(AgentTransportKind {
    StructuredStdio,
    LocalHttp,
    EmbeddedSdk,
    TerminalFallback
});
wire_enum!(CapabilitySupport {
    Supported,
    Unsupported,
    Unknown,
    Conditional
});
wire_enum!(CapabilityAccess { Observe, Request });
wire_enum!(CapabilityId {
    SessionCreate,
    SessionResume,
    SessionInterrupt,
    SessionCancel,
    SessionStatus,
    PromptSubmit,
    StructuredMessages,
    StreamingOutput,
    ModelList,
    ModelSelection,
    ToolCalls,
    ToolResults,
    Approvals,
    Subagents,
    Skills,
    Hooks,
    McpServers,
    TokenUsage,
    CostUsage,
    Latency,
    WorkingDirectory,
    TerminalIo
});
wire_enum!(EvidenceKind {
    StaticKnowledge,
    RuntimeHandshake,
    RuntimeProbe,
    UserConfiguration,
    FallbackAssumption
});
wire_enum!(EvidenceOutcome {
    Declared,
    Confirmed,
    Inconclusive
});
wire_enum!(CompatibilityState {
    Compatible,
    Incompatible,
    Degraded,
    Unknown
});
wire_enum!(CapabilityReason {
    RuntimeVersionUnknown,
    TransportLacksStructure,
    RequiresConfiguration,
    RuntimeDoesNotExpose,
    NotImplementedByAdapter,
    NotProbed,
    PartialCoverage,
    ProbeFailed
});
wire_enum!(ManagedAgentSessionState {
    Unknown,
    Creating,
    Idle,
    Running,
    WaitingForInput,
    WaitingForApproval,
    Interrupted,
    Completed,
    Failed,
    Closed
});
wire_enum!(ModelAvailability {
    Available,
    Unavailable,
    Unknown
});
wire_enum!(AdapterErrorCode {
    Unavailable,
    Unsupported,
    IncompatibleRuntime,
    ProtocolError,
    TransportError,
    AuthenticationRequired,
    PermissionRequired,
    InvalidRequest,
    RuntimeFailed,
    Interrupted,
    Timeout
});

macro_rules! machine_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl $name {
            pub fn parse(value: &str) -> Result<Self, AdapterContractError> {
                validation::machine_id(value)?;
                Ok(Self(value.to_owned()))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl TryFrom<String> for $name {
            type Error = AdapterContractError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(&value)
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}
machine_id!(AgentAdapterId);
machine_id!(ModelProviderId);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterContractError {
    pub code: &'static str,
}
impl fmt::Display for AdapterContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code)
    }
}
impl std::error::Error for AdapterContractError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityEvidence {
    kind: EvidenceKind,
    outcome: EvidenceOutcome,
    reference: Option<String>,
    observed_at: Option<UtcTimestamp>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionObservation {
    value: Option<String>,
    evidence: CapabilityEvidence,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityDeclaration {
    support: CapabilitySupport,
    evidence: Vec<CapabilityEvidence>,
    conditions: Vec<String>,
    reason: Option<CapabilityReason>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterCompatibility {
    state: CompatibilityState,
    evidence: Vec<CapabilityEvidence>,
    reason: Option<CapabilityReason>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SafeDiagnostic {
    code: String,
    summary: String,
}

// One field definition generates a private, validated root and a temporary wire
// decoder. Deserialize cannot bypass validation; callers receive shared borrows.
macro_rules! contract {
    ($name:ident, $validator:ident, { $($field:ident: $ty:ty),+ $(,)? }) => {
        #[derive(Debug, Clone, Serialize)]
        pub struct $name { $($field: $ty),+ }
        impl $name {
            pub fn from_value(mut value: Value) -> Result<Self, AdapterContractError> {
                validation::$validator(&value)?;
                value["schema_version"] = serde_json::json!(1);
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Wire { $($field: $ty),+ }
                let wire: Wire = serde_json::from_value(value)
                    .map_err(|_| AdapterContractError { code: "invalid_contract" })?;
                Ok(Self { $($field: wire.$field),+ })
            }
            pub fn validate(&self) -> Result<(), AdapterContractError> {
                let value = serde_json::to_value(self)
                    .map_err(|_| AdapterContractError { code: "invalid_contract" })?;
                validation::$validator(&value)
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                Self::from_value(Value::deserialize(d)?).map_err(serde::de::Error::custom)
            }
        }
    };
}
contract!(AgentAdapterDescriptorV1, descriptor, {
    schema_version: u8,
    adapter_id: AgentAdapterId,
    runtime_kind: AgentRuntimeKind,
    implementation_version: String,
    runtime_version: VersionObservation,
    protocol_version: VersionObservation,
    transport: AgentTransportKind,
    capabilities: BTreeMap<CapabilityId, BTreeMap<CapabilityAccess, CapabilityDeclaration>>,
    compatibility: AdapterCompatibility,
    limitations: Vec<String>,
    extensions: BTreeMap<String, Map<String, Value>>,
});
impl AgentAdapterDescriptorV1 {
    pub fn adapter_id(&self) -> &AgentAdapterId {
        &self.adapter_id
    }
    /// Absence is unknown, never an implicit supported grant.
    pub fn support(&self, capability: CapabilityId, access: CapabilityAccess) -> CapabilitySupport {
        self.capabilities
            .get(&capability)
            .and_then(|entry| entry.get(&access))
            .map_or(CapabilitySupport::Unknown, |entry| entry.support)
    }
    pub fn declaration(
        &self,
        capability: CapabilityId,
        access: CapabilityAccess,
    ) -> Option<&CapabilityDeclaration> {
        self.capabilities
            .get(&capability)
            .and_then(|entry| entry.get(&access))
    }
}
contract!(AgentModelDescriptorV1, model, {
    schema_version: u8,
    provider_id: Option<ModelProviderId>,
    model_id: String,
    display_name: Option<String>,
    availability: ModelAvailability,
});
contract!(ManagedAgentSessionObservationV1, session, {
    schema_version: u8,
    managed_session_id: ManagedSessionId,
    adapter_id: AgentAdapterId,
    runtime_instance_id: AgentRuntimeInstanceId,
    runtime_session_id: Option<String>,
    state: ManagedAgentSessionState,
    runtime_state: Option<String>,
    observed_model: Option<AgentModelDescriptorV1>,
    terminal_id: Option<TerminalId>,
});
impl ManagedAgentSessionObservationV1 {
    pub fn managed_session_id(&self) -> &ManagedSessionId {
        &self.managed_session_id
    }
    pub fn runtime_instance_id(&self) -> &AgentRuntimeInstanceId {
        &self.runtime_instance_id
    }
    pub fn state(&self) -> ManagedAgentSessionState {
        self.state
    }
}
contract!(AdapterErrorV1, adapter_error, {
    schema_version: u8,
    code: AdapterErrorCode,
    operation: Option<CapabilityId>,
    diagnostic: Option<SafeDiagnostic>,
});
