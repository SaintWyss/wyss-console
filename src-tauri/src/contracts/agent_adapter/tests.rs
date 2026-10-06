use super::*;

fn corpus() -> Value {
    serde_json::from_str(include_str!(
        "../../../../tests/fixtures/agent-adapter-v1.json"
    ))
    .unwrap()
}
fn round_trip<T: serde::de::DeserializeOwned + Serialize>(value: Value) -> Value {
    let contract: T = serde_json::from_value(value).unwrap();
    serde_json::to_value(contract).unwrap()
}
fn parse(kind: &str, value: Value) -> Result<Value, AdapterContractError> {
    macro_rules! dto {
        ($ty:ty) => {{
            let dto = <$ty>::from_value(value)?;
            dto.validate()?;
            let serialized = serde_json::to_value(dto).unwrap();
            assert_eq!(round_trip::<$ty>(serialized.clone()), serialized);
            Ok(serialized)
        }};
    }
    match kind {
        "adapter" => dto!(AgentAdapterDescriptorV1),
        "session" => dto!(ManagedAgentSessionObservationV1),
        "model" => dto!(AgentModelDescriptorV1),
        "error" => dto!(AdapterErrorV1),
        _ => panic!("unknown fixture kind"),
    }
}

#[test]
fn shared_valid_corpus_semantically_round_trips() {
    for case in corpus()["valid"].as_array().unwrap() {
        let value = case["value"].clone();
        let result = parse(case["kind"].as_str().unwrap(), value.clone())
            .unwrap_or_else(|error| panic!("{}: {error}", case["name"]));
        // JSON 1.0 and 1 are semantically the same schema discriminator.
        let mut original = value;
        original["schema_version"] = serde_json::json!(1);
        assert_eq!(result, original, "{}", case["name"]);
    }
}

#[test]
fn shared_invalid_corpus_rejects_in_both_entrypoints() {
    for case in corpus()["invalid"].as_array().unwrap() {
        let value = case["value"].clone();
        let kind = case["kind"].as_str().unwrap();
        let error = parse(kind, value.clone()).unwrap_err();
        assert_eq!(
            error.code,
            case["error"].as_str().unwrap(),
            "{}",
            case["name"]
        );
        let rejected = match kind {
            "adapter" => serde_json::from_value::<AgentAdapterDescriptorV1>(value).is_err(),
            "session" => serde_json::from_value::<ManagedAgentSessionObservationV1>(value).is_err(),
            "model" => serde_json::from_value::<AgentModelDescriptorV1>(value).is_err(),
            "error" => serde_json::from_value::<AdapterErrorV1>(value).is_err(),
            _ => unreachable!(),
        };
        assert!(rejected, "{}", case["name"]);
    }
}

#[test]
fn absent_capabilities_never_imply_support() {
    let dto = AgentAdapterDescriptorV1::from_value(corpus()["valid"][0]["value"].clone()).unwrap();
    assert_eq!(dto.adapter_id().as_str(), "codex");
    assert_eq!(
        dto.support(CapabilityId::ModelSelection, CapabilityAccess::Observe),
        CapabilitySupport::Supported
    );
    assert_eq!(
        dto.support(CapabilityId::ModelSelection, CapabilityAccess::Request),
        CapabilitySupport::Unknown
    );
    assert_eq!(
        dto.support(CapabilityId::SessionResume, CapabilityAccess::Request),
        CapabilitySupport::Unknown
    );
    assert!(
        dto.declaration(CapabilityId::SessionResume, CapabilityAccess::Request)
            .is_none()
    );
}

#[test]
fn terminal_fallback_has_no_positive_structured_coverage() {
    let dto = AgentAdapterDescriptorV1::from_value(corpus()["valid"][3]["value"].clone()).unwrap();
    assert_eq!(
        dto.support(CapabilityId::TerminalIo, CapabilityAccess::Request),
        CapabilitySupport::Supported
    );
    assert_eq!(
        dto.support(CapabilityId::ToolCalls, CapabilityAccess::Observe),
        CapabilitySupport::Unsupported
    );
    assert_eq!(
        dto.support(CapabilityId::Approvals, CapabilityAccess::Request),
        CapabilitySupport::Unknown
    );
}

#[test]
fn session_is_observed_and_provider_is_independent_from_adapter() {
    let value = corpus()["valid"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == "session-idle")
        .unwrap()["value"]
        .clone();
    let dto = ManagedAgentSessionObservationV1::from_value(value.clone()).unwrap();
    assert_eq!(dto.state(), ManagedAgentSessionState::Idle);
    assert_ne!(
        dto.managed_session_id().as_str(),
        dto.runtime_instance_id().as_str()
    );
    assert_eq!(value["adapter_id"], "opencode");
    assert_eq!(value["observed_model"]["provider_id"], "anthropic");
}

#[test]
fn machine_identity_is_not_a_runtime_instance() {
    assert!(AgentAdapterId::parse("claude-code").is_ok());
    assert!(AgentAdapterId::parse("CLAUDE").is_err());
    assert!(ModelProviderId::parse("openai").is_ok());
    assert!(AgentRuntimeInstanceId::parse("codex").is_err());
}
