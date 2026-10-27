//! Strict protocol-2 workspace-guard frames. These methods are a closed F8 extension and do not
//! enter the older protocol-1 operation enum or its authorization surface.
use crate::contracts::{
    WireUuid,
    control::{ControlError as E, ControlResult, unique_json},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Digest;
use std::path::PathBuf;

#[derive(Debug)]
pub struct Request {
    pub request_id: WireUuid,
    pub authority_instance_id: WireUuid,
    pub operation: Operation,
}

#[derive(Debug)]
pub enum Operation {
    Capabilities,
    ImportClaims {
        coordinator_store_id: String,
        snapshot_revision: u64,
        claims: Vec<super::workspace_guard::ImportedClaim>,
    },
    ObserveRoot {
        locator: PathBuf,
    },
    AcquireClaim {
        coordinator_store_id: String,
        run_id: String,
        plan_revision: u64,
        plan_digest: String,
        root_binding_id: String,
    },
    SetGate {
        claim_id: String,
        expected_claim_revision: u64,
        expected_gate_revision: u64,
        gate: String,
        settlement_id: Option<String>,
        coordinator_revision: Option<u64>,
    },
    ClaimStatus {
        claim_id: String,
    },
    VerificationPrepare(VerificationPrepare),
    VerificationCommit(VerificationCommit),
    ClaimRelease(ClaimRelease),
    ClaimReleaseUnused(ClaimReleaseUnused),
    StartPrepare(StartTuple),
    StartCommit(StartCommit),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationDecision {
    ConfirmedCompleted,
    ConfirmedNoEffect,
    ConfirmedCancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VerificationPrepare {
    pub observation_id: String,
    pub claim_id: String,
    pub claim_generation: u64,
    pub expected_claim_revision: u64,
    pub root_binding_id: String,
    pub attempt_id: String,
    pub locator: PathBuf,
    pub reviewed_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VerificationCommit {
    pub claim_id: String,
    pub claim_generation: u64,
    pub expected_claim_revision: u64,
    pub observation_id: String,
    pub attempt_id: String,
    pub locator: PathBuf,
    pub decision: VerificationDecision,
    pub reviewed_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ClaimRelease {
    pub claim_id: String,
    pub claim_generation: u64,
    pub expected_claim_revision: u64,
    pub settlement_id: String,
    pub coordinator_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ClaimReleaseUnused {
    pub claim_id: String,
    pub claim_generation: u64,
    pub expected_claim_revision: u64,
    pub expected_gate_revision: u64,
    pub coordinator_revision: u64,
}

/// The immutable M6 intent and the F8 state it must still match at both preparation and L1.
/// This is metadata only; prompt bytes are accepted only by `StartCommit` and never persisted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StartTuple {
    pub authority_store_id: String,
    pub authority_epoch: String,
    pub coordinator_store_id: String,
    pub run_id: String,
    pub plan_revision: u64,
    pub plan_digest: String,
    pub step_id: String,
    pub attempt_id: String,
    pub intent_id: String,
    pub effect_class: StartEffect,
    pub prompt_digest: String,
    pub model_id: Option<String>,
    pub model_profile: StartProfile,
    pub root_binding_id: String,
    pub claim_id: String,
    pub claim_generation: u64,
    pub claim_revision: u64,
    pub root_observation_revision: u64,
    pub dispatch_gate_revision: u64,
    pub session_id: String,
    pub session_revision: u64,
    pub attachment_revision: u64,
    pub runtime_instance_id: String,
    pub runtime_generation: u64,
    pub lease_id: String,
    pub lease_revision: u64,
    pub approval_reference: Option<String>,
    pub purpose: StartPurpose,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StartEffect {
    ManagedInference,
    ManagedLocalTools,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StartProfile {
    CodexInferenceV1,
    CodexLocalToolsV1,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StartPurpose {
    PipelineStep,
    HandoffFreshReceiver,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartCommit {
    pub tuple: StartTuple,
    pub receipt_id: String,
    pub receipt_revision: u64,
    pub prompt: String,
}

impl std::fmt::Debug for StartCommit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StartCommit(<sensitive prompt>)")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    schema_version: u8,
    request_id: WireUuid,
    authority_instance_id: WireUuid,
    method: String,
    params: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportClaims {
    coordinator_store_id: String,
    snapshot_revision: u64,
    claims: Vec<super::workspace_guard::ImportedClaim>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ObserveRoot {
    locator: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AcquireClaim {
    coordinator_store_id: String,
    run_id: String,
    plan_revision: u64,
    plan_digest: String,
    root_binding_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetGate {
    claim_id: String,
    expected_claim_revision: u64,
    expected_gate_revision: u64,
    gate: String,
    #[serde(default)]
    settlement_id: Option<String>,
    #[serde(default)]
    coordinator_revision: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaimStatus {
    claim_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartPrepare {
    tuple: StartTuple,
}

impl Request {
    pub fn parse(bytes: &[u8]) -> ControlResult<Self> {
        let value = unique_json(bytes)?;
        let frame: Frame = serde_json::from_value(value).map_err(|_| E::InvalidRequest)?;
        if frame.schema_version != 2 {
            return Err(E::ProtocolVersionUnsupported);
        }
        for id in [
            frame.request_id.as_str(),
            frame.authority_instance_id.as_str(),
        ] {
            super::protocol::canonical(id)?;
        }
        let operation = match frame.method.as_str() {
            "authority/capabilities" => {
                serde_json::from_value::<Empty>(frame.params).map_err(|_| E::InvalidRequest)?;
                Operation::Capabilities
            }
            "orchestration/claims/import" => {
                let p: ImportClaims =
                    serde_json::from_value(frame.params).map_err(|_| E::InvalidRequest)?;
                Operation::ImportClaims {
                    coordinator_store_id: p.coordinator_store_id,
                    snapshot_revision: p.snapshot_revision,
                    claims: p.claims,
                }
            }
            "orchestration/root/observe" => {
                let p: ObserveRoot =
                    serde_json::from_value(frame.params).map_err(|_| E::InvalidRequest)?;
                Operation::ObserveRoot { locator: p.locator }
            }
            "orchestration/claim/acquire" => {
                let p: AcquireClaim =
                    serde_json::from_value(frame.params).map_err(|_| E::InvalidRequest)?;
                Operation::AcquireClaim {
                    coordinator_store_id: p.coordinator_store_id,
                    run_id: p.run_id,
                    plan_revision: p.plan_revision,
                    plan_digest: p.plan_digest,
                    root_binding_id: p.root_binding_id,
                }
            }
            "orchestration/claim/gate" => {
                let p: SetGate =
                    serde_json::from_value(frame.params).map_err(|_| E::InvalidRequest)?;
                Operation::SetGate {
                    claim_id: p.claim_id,
                    expected_claim_revision: p.expected_claim_revision,
                    expected_gate_revision: p.expected_gate_revision,
                    gate: p.gate,
                    settlement_id: p.settlement_id,
                    coordinator_revision: p.coordinator_revision,
                }
            }
            "orchestration/claim/status" => {
                let p: ClaimStatus =
                    serde_json::from_value(frame.params).map_err(|_| E::InvalidRequest)?;
                super::protocol::canonical(&p.claim_id)?;
                Operation::ClaimStatus {
                    claim_id: p.claim_id,
                }
            }
            "orchestration/verification/prepare" => {
                let p: VerificationPrepare =
                    serde_json::from_value(frame.params).map_err(|_| E::InvalidRequest)?;
                validate_verification_ids(
                    [
                        &p.observation_id,
                        &p.claim_id,
                        &p.root_binding_id,
                        &p.attempt_id,
                    ],
                    p.claim_generation,
                    p.expected_claim_revision,
                    &p.reviewed_digest,
                )?;
                Operation::VerificationPrepare(p)
            }
            "orchestration/verification/commit" => {
                let p: VerificationCommit =
                    serde_json::from_value(frame.params).map_err(|_| E::InvalidRequest)?;
                validate_verification_ids(
                    [&p.claim_id, &p.observation_id, &p.attempt_id],
                    p.claim_generation,
                    p.expected_claim_revision,
                    &p.reviewed_digest,
                )?;
                Operation::VerificationCommit(p)
            }
            "orchestration/claim/release" => {
                let p: ClaimRelease =
                    serde_json::from_value(frame.params).map_err(|_| E::InvalidRequest)?;
                for id in [&p.claim_id, &p.settlement_id] {
                    super::protocol::canonical(id)?;
                }
                if p.claim_generation == 0
                    || p.expected_claim_revision == 0
                    || p.coordinator_revision == 0
                {
                    return Err(E::InvalidRequest);
                }
                Operation::ClaimRelease(p)
            }
            "orchestration/claim/release-unused" => {
                let p: ClaimReleaseUnused =
                    serde_json::from_value(frame.params).map_err(|_| E::InvalidRequest)?;
                super::protocol::canonical(&p.claim_id)?;
                if p.claim_generation == 0
                    || p.expected_claim_revision == 0
                    || p.expected_gate_revision == 0
                    || p.coordinator_revision == 0
                {
                    return Err(E::InvalidRequest);
                }
                Operation::ClaimReleaseUnused(p)
            }
            "orchestration/start/prepare" => {
                let p: StartPrepare =
                    serde_json::from_value(frame.params).map_err(|_| E::InvalidRequest)?;
                p.tuple.validate()?;
                Operation::StartPrepare(p.tuple)
            }
            "orchestration/start/commit" => {
                let p: StartCommit =
                    serde_json::from_value(frame.params).map_err(|_| E::InvalidRequest)?;
                p.tuple.validate()?;
                p.validate()?;
                Operation::StartCommit(p)
            }
            _ => return Err(E::UnsupportedMethod),
        };
        Ok(Self {
            request_id: frame.request_id,
            authority_instance_id: frame.authority_instance_id,
            operation,
        })
    }
}

fn validate_verification_ids<const N: usize>(
    ids: [&String; N],
    generation: u64,
    claim_revision: u64,
    digest: &str,
) -> ControlResult<()> {
    for id in ids {
        super::protocol::canonical(id)?;
    }
    if generation == 0
        || claim_revision == 0
        || digest.len() != 64
        || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(E::InvalidRequest);
    }
    Ok(())
}

impl StartTuple {
    /// Stable digest for the closed, serialized tuple. It contains metadata only; the prompt is
    /// separately checked against `prompt_digest` and is never part of this durable identity.
    pub fn digest(&self) -> ControlResult<String> {
        let encoded = serde_json::to_vec(self).map_err(|_| E::InvalidRequest)?;
        Ok(format!("{:x}", sha2::Sha256::digest(encoded)))
    }

    pub fn validate(&self) -> ControlResult<()> {
        for id in [
            &self.authority_store_id,
            &self.authority_epoch,
            &self.coordinator_store_id,
            &self.run_id,
            &self.step_id,
            &self.attempt_id,
            &self.intent_id,
            &self.root_binding_id,
            &self.claim_id,
            &self.session_id,
            &self.runtime_instance_id,
            &self.lease_id,
        ] {
            super::protocol::canonical(id)?;
        }
        if self
            .approval_reference
            .as_ref()
            .is_some_and(|id| super::protocol::canonical(id).is_err())
        {
            return Err(E::InvalidRequest);
        }
        for revision in [
            self.plan_revision,
            self.claim_generation,
            self.claim_revision,
            self.root_observation_revision,
            self.dispatch_gate_revision,
            self.session_revision,
            self.attachment_revision,
            self.runtime_generation,
            self.lease_revision,
        ] {
            if revision == 0 || revision > i64::MAX as u64 {
                return Err(E::InvalidRequest);
            }
        }
        for digest in [&self.plan_digest, &self.prompt_digest] {
            if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(E::InvalidRequest);
            }
        }
        if self.effect_class == StartEffect::ManagedInference
            && self.model_profile != StartProfile::CodexInferenceV1
            || self.effect_class == StartEffect::ManagedLocalTools
                && self.model_profile != StartProfile::CodexLocalToolsV1
        {
            return Err(E::InvalidRequest);
        }
        Ok(())
    }
}

impl StartCommit {
    fn validate(&self) -> ControlResult<()> {
        super::protocol::canonical(&self.receipt_id)?;
        if self.receipt_revision == 0
            || self.receipt_revision > i64::MAX as u64
            || self.prompt.is_empty()
            || self.prompt.len() > 8192
            || format!("{:x}", sha2::Sha256::digest(self.prompt.as_bytes()))
                != self.tuple.prompt_digest
        {
            return Err(E::InvalidRequest);
        }
        Ok(())
    }
}

pub fn success(
    request_id: &WireUuid,
    authority: &WireUuid,
    value: Value,
) -> ControlResult<Vec<u8>> {
    let bytes = serde_json::to_vec(&serde_json::json!({
        "schema_version": 2,
        "request_id": request_id,
        "authority_instance_id": authority,
        "result": {"kind": "workspace_guard", "value": value}
    }))
    .map_err(|_| E::InternalError)?;
    if bytes.len() > crate::contracts::control::MAX_FRAME {
        return Err(E::FrameTooLarge);
    }
    Ok(bytes)
}

pub fn response_value(
    bytes: &[u8],
    request_id: &WireUuid,
    authority: &WireUuid,
) -> ControlResult<Value> {
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
        exact(&v["result"], &["kind", "value"])?;
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
        exact(&v["error"], &["code"])?;
    }
    if v["schema_version"].as_u64() != Some(2)
        || v["request_id"] != serde_json::json!(request_id)
        || v["authority_instance_id"] != serde_json::json!(authority)
    {
        return Err(E::CorrelationError);
    }
    if let Some(error) = v.get("error") {
        let code: E = serde_json::from_value(error.get("code").cloned().ok_or(E::InvalidRequest)?)
            .map_err(|_| E::InvalidRequest)?;
        return Err(code);
    }
    if v["result"]["kind"] != "workspace_guard" {
        return Err(E::InvalidRequest);
    }
    v["result"]["value"].as_object().ok_or(E::InvalidRequest)?;
    Ok(v["result"]["value"].clone())
}

fn exact(value: &Value, keys: &[&str]) -> ControlResult<()> {
    let object = value.as_object().ok_or(E::InvalidRequest)?;
    if object.len() != keys.len() || keys.iter().any(|key| !object.contains_key(*key)) {
        return Err(E::InvalidRequest);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuple() -> StartTuple {
        let id = "00000000-0000-4000-8000-000000000001".to_owned();
        StartTuple {
            authority_store_id: id.clone(),
            authority_epoch: id.clone(),
            coordinator_store_id: id.clone(),
            run_id: id.clone(),
            plan_revision: 1,
            plan_digest: "a".repeat(64),
            step_id: id.clone(),
            attempt_id: id.clone(),
            intent_id: id.clone(),
            effect_class: StartEffect::ManagedInference,
            prompt_digest: "b".repeat(64),
            model_id: None,
            model_profile: StartProfile::CodexInferenceV1,
            root_binding_id: id.clone(),
            claim_id: id.clone(),
            claim_generation: 1,
            claim_revision: 1,
            root_observation_revision: 1,
            dispatch_gate_revision: 1,
            session_id: id.clone(),
            session_revision: 1,
            attachment_revision: 1,
            runtime_instance_id: id.clone(),
            runtime_generation: 1,
            lease_id: id,
            lease_revision: 1,
            approval_reference: None,
            purpose: StartPurpose::PipelineStep,
        }
    }

    fn request(method: &str, params: Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "schema_version":2,
            "request_id":"00000000-0000-4000-8000-000000000002",
            "authority_instance_id":"00000000-0000-4000-8000-000000000003",
            "method":method,
            "params":params
        }))
        .unwrap()
    }

    #[test]
    fn guarded_start_contract_is_versioned_closed_and_binds_effect_profile() {
        let valid_tuple = tuple();
        assert!(matches!(
            Request::parse(&request(
                "orchestration/start/prepare",
                serde_json::json!({"tuple":valid_tuple})
            ))
            .unwrap()
            .operation,
            Operation::StartPrepare(_)
        ));

        let mut mismatched = tuple();
        mismatched.effect_class = StartEffect::ManagedLocalTools;
        assert_eq!(mismatched.validate(), Err(E::InvalidRequest));

        let mut unknown = serde_json::to_value(tuple()).unwrap();
        unknown["force"] = Value::Bool(true);
        assert!(
            Request::parse(&request(
                "orchestration/start/prepare",
                serde_json::json!({"tuple":unknown})
            ))
            .is_err()
        );

        let mut commit_tuple = tuple();
        commit_tuple.prompt_digest = format!("{:x}", sha2::Sha256::digest(b"synthetic prompt"));
        let commit = request(
            "orchestration/start/commit",
            serde_json::json!({
                "tuple":commit_tuple,
                "receipt_id":"00000000-0000-4000-8000-000000000004",
                "receipt_revision":1,
                "prompt":"synthetic prompt"
            }),
        );
        assert!(matches!(
            Request::parse(&commit).unwrap().operation,
            Operation::StartCommit(_)
        ));
        assert!(
            !format!("{:?}", Request::parse(&commit).unwrap().operation)
                .contains("synthetic prompt")
        );
    }

    #[test]
    fn settlement_contracts_are_closed_and_require_identity_revisions() {
        let id = "00000000-0000-4000-8000-000000000010";
        let prepare = request(
            "orchestration/verification/prepare",
            serde_json::json!({
                "observation_id":"00000000-0000-4000-8000-000000000014",
                "claim_id":id,
                "claim_generation":1,
                "expected_claim_revision":2,
                "root_binding_id":"00000000-0000-4000-8000-000000000011",
                "attempt_id":"00000000-0000-4000-8000-000000000012",
                "locator":"/tmp/workspace",
                "reviewed_digest":"a".repeat(64)
            }),
        );
        assert!(matches!(
            Request::parse(&prepare).unwrap().operation,
            Operation::VerificationPrepare(_)
        ));

        let release = request(
            "orchestration/claim/release",
            serde_json::json!({
                "claim_id":id,
                "claim_generation":1,
                "expected_claim_revision":3,
                "settlement_id":"00000000-0000-4000-8000-000000000013",
                "coordinator_revision":9
            }),
        );
        assert!(matches!(
            Request::parse(&release).unwrap().operation,
            Operation::ClaimRelease(_)
        ));

        let commit = request(
            "orchestration/verification/commit",
            serde_json::json!({
                "claim_id":id,
                "claim_generation":1,
                "expected_claim_revision":2,
                "observation_id":"00000000-0000-4000-8000-000000000014",
                "attempt_id":"00000000-0000-4000-8000-000000000012",
                "locator":"/tmp/workspace",
                "decision":"confirmed_completed",
                "reviewed_digest":"a".repeat(64)
            }),
        );
        assert!(matches!(
            Request::parse(&commit).unwrap().operation,
            Operation::VerificationCommit(_)
        ));

        let mut unknown = serde_json::json!({
            "observation_id":"00000000-0000-4000-8000-000000000014",
            "claim_id":id,
            "claim_generation":1,
            "expected_claim_revision":2,
            "root_binding_id":"00000000-0000-4000-8000-000000000011",
            "attempt_id":"00000000-0000-4000-8000-000000000012",
            "locator":"/tmp/workspace",
            "reviewed_digest":"a".repeat(64)
        });
        unknown["force_release"] = Value::Bool(true);
        assert!(Request::parse(&request("orchestration/verification/prepare", unknown)).is_err());

        let bad_revision = request(
            "orchestration/claim/release",
            serde_json::json!({
                "claim_id":id,
                "claim_generation":1,
                "expected_claim_revision":0,
                "settlement_id":"00000000-0000-4000-8000-000000000013",
                "coordinator_revision":9
            }),
        );
        assert!(Request::parse(&bad_revision).is_err());
    }
}
