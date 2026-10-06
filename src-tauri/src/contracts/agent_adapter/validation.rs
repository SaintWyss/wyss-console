use super::*;

type Result<T = ()> = std::result::Result<T, AdapterContractError>;
fn ensure(condition: bool, code: &'static str) -> Result {
    if condition {
        Ok(())
    } else {
        Err(AdapterContractError { code })
    }
}
fn object(value: &Value) -> Result<&Map<String, Value>> {
    value.as_object().ok_or(AdapterContractError {
        code: "expected_object",
    })
}
fn fields<'a>(value: &'a Value, names: &[&str]) -> Result<&'a Map<String, Value>> {
    let map = object(value)?;
    ensure(
        names.iter().all(|key| map.contains_key(*key)),
        "missing_field",
    )?;
    ensure(
        map.keys().all(|key| names.contains(&key.as_str())),
        "unexpected_field",
    )?;
    Ok(map)
}
fn text(value: &Value, max: usize) -> Result<&str> {
    let s = value.as_str().ok_or(AdapterContractError {
        code: "invalid_text",
    })?;
    ensure(
        !s.trim_matches(' ').is_empty() && s.len() <= max && !s.chars().any(char::is_control),
        "invalid_text",
    )?;
    Ok(s)
}
fn optional_text(value: &Value, max: usize) -> Result {
    if value.is_null() {
        Ok(())
    } else {
        text(value, max).map(|_| ())
    }
}
fn enum_value<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone()).map_err(|_| AdapterContractError {
        code: "invalid_enum",
    })
}
pub(super) fn machine_id(s: &str) -> Result {
    let bytes = s.as_bytes();
    ensure(
        !bytes.is_empty()
            && bytes.len() <= 64
            && bytes[0].is_ascii_lowercase()
            && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
            && bytes
                .iter()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
            && !s.contains("--"),
        "invalid_id",
    )
}
fn id(value: &Value) -> Result {
    machine_id(text(value, 64)?)
}
fn namespace(value: &str) -> bool {
    value.len() <= 96
        && value.contains('.')
        && value.split('.').all(|segment| {
            !segment.is_empty()
                && segment.as_bytes()[0].is_ascii_lowercase()
                && segment.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-')
                })
        })
        && !matches!(
            value.split('.').next(),
            Some("wyss" | "core" | "normalized")
        )
}
fn uuid(value: &Value) -> Result {
    WireUuid::try_from(text(value, 36)?.to_owned())
        .map(|_| ())
        .map_err(|_| AdapterContractError {
            code: "invalid_uuid",
        })
}
fn bounded(value: &Value, depth: usize, nodes: &mut usize, bytes: &mut usize) -> Result {
    *nodes += 1;
    ensure(depth <= 12 && *nodes <= 2048, "limit_exceeded")?;
    match value {
        Value::String(s) => {
            ensure(s.len() <= 2048, "limit_exceeded")?;
            *bytes += s.len();
        }
        Value::Array(items) => {
            ensure(items.len() <= 64, "limit_exceeded")?;
            for item in items {
                bounded(item, depth + 1, nodes, bytes)?;
            }
        }
        Value::Object(map) => {
            ensure(map.len() <= 64, "limit_exceeded")?;
            for (key, item) in map {
                ensure(key.len() <= 128, "limit_exceeded")?;
                *bytes += key.len();
                bounded(item, depth + 1, nodes, bytes)?;
            }
        }
        Value::Number(n) => {
            let n = n.as_f64().ok_or(AdapterContractError {
                code: "invalid_json",
            })?;
            ensure(
                n.is_finite() && (n.fract() != 0.0 || n.abs() <= 9_007_199_254_740_991.0),
                "invalid_json",
            )?;
        }
        _ => {}
    }
    ensure(*bytes <= 32768, "limit_exceeded")
}
fn root(value: &Value, names: &[&str]) -> Result {
    bounded(value, 0, &mut 0, &mut 0)?;
    let map = fields(value, names)?;
    ensure(
        map["schema_version"].as_f64() == Some(1.0),
        "invalid_schema_version",
    )
}
fn evidence(value: &Value) -> Result<EvidenceOutcome> {
    let map = fields(value, &["kind", "outcome", "reference", "observed_at"])?;
    let kind = enum_value::<EvidenceKind>(&map["kind"])?;
    let outcome = enum_value::<EvidenceOutcome>(&map["outcome"])?;
    optional_text(&map["reference"], 512)?;
    if !map["observed_at"].is_null() {
        UtcTimestamp::try_from(text(&map["observed_at"], 64)?.to_owned()).map_err(|_| {
            AdapterContractError {
                code: "invalid_timestamp",
            }
        })?;
    }
    if outcome == EvidenceOutcome::Confirmed {
        ensure(
            matches!(
                kind,
                EvidenceKind::RuntimeHandshake | EvidenceKind::RuntimeProbe
            ) && !map["observed_at"].is_null(),
            "contradictory_evidence",
        )?;
    }
    Ok(outcome)
}
fn evidence_list(value: &Value) -> Result<Vec<EvidenceOutcome>> {
    let list = value.as_array().ok_or(AdapterContractError {
        code: "expected_array",
    })?;
    ensure(!list.is_empty() && list.len() <= 8, "invalid_evidence")?;
    list.iter().map(evidence).collect()
}
fn version(value: &Value) -> Result {
    let map = fields(value, &["value", "evidence"])?;
    optional_text(&map["value"], 128)?;
    let outcome = evidence(&map["evidence"])?;
    ensure(
        !(map["value"].is_null() && outcome == EvidenceOutcome::Confirmed),
        "contradictory_evidence",
    )
}
fn reason(value: &Value) -> Result {
    if value.is_null() {
        Ok(())
    } else {
        enum_value::<CapabilityReason>(value).map(|_| ())
    }
}
fn declaration(value: &Value) -> Result<CapabilitySupport> {
    let map = fields(value, &["support", "evidence", "conditions", "reason"])?;
    let support = enum_value::<CapabilitySupport>(&map["support"])?;
    let outcomes = evidence_list(&map["evidence"])?;
    reason(&map["reason"])?;
    let conditions = map["conditions"].as_array().ok_or(AdapterContractError {
        code: "expected_array",
    })?;
    ensure(conditions.len() <= 8, "limit_exceeded")?;
    for condition in conditions {
        ensure(namespace(text(condition, 96)?), "invalid_namespace")?;
    }
    ensure(
        (support == CapabilitySupport::Conditional) != conditions.is_empty(),
        "contradictory_capability",
    )?;
    ensure(
        support == CapabilitySupport::Supported || !map["reason"].is_null(),
        "contradictory_capability",
    )?;
    ensure(
        support != CapabilitySupport::Unknown || !outcomes.contains(&EvidenceOutcome::Confirmed),
        "contradictory_capability",
    )?;
    ensure(
        !matches!(
            support,
            CapabilitySupport::Supported | CapabilitySupport::Conditional
        ) || outcomes.iter().any(|o| *o != EvidenceOutcome::Inconclusive),
        "contradictory_capability",
    )?;
    Ok(support)
}
fn access_allowed(id: CapabilityId, access: CapabilityAccess) -> bool {
    use CapabilityId::*;
    match id {
        SessionCreate | SessionResume | SessionInterrupt | SessionCancel | PromptSubmit => {
            access == CapabilityAccess::Request
        }
        SessionStatus | StructuredMessages | StreamingOutput | ModelList | ToolCalls
        | ToolResults | Hooks | McpServers | TokenUsage | CostUsage | Latency => {
            access == CapabilityAccess::Observe
        }
        ModelSelection | Approvals | Subagents | Skills | WorkingDirectory | TerminalIo => true,
    }
}
pub(super) fn descriptor(value: &Value) -> Result {
    root(
        value,
        &[
            "schema_version",
            "adapter_id",
            "runtime_kind",
            "implementation_version",
            "runtime_version",
            "protocol_version",
            "transport",
            "capabilities",
            "compatibility",
            "limitations",
            "extensions",
        ],
    )?;
    let map = object(value)?;
    id(&map["adapter_id"])?;
    enum_value::<AgentRuntimeKind>(&map["runtime_kind"])?;
    text(&map["implementation_version"], 128)?;
    version(&map["runtime_version"])?;
    version(&map["protocol_version"])?;
    let transport = enum_value::<AgentTransportKind>(&map["transport"])?;
    let capabilities = object(&map["capabilities"])?;
    for (key, coverage) in capabilities {
        let capability = enum_value::<CapabilityId>(&Value::String(key.clone()))?;
        let coverage = object(coverage)?;
        ensure(!coverage.is_empty(), "contradictory_capability")?;
        for (key, value) in coverage {
            let access = enum_value::<CapabilityAccess>(&Value::String(key.clone()))?;
            ensure(
                access_allowed(capability, access),
                "contradictory_capability",
            )?;
            let support = declaration(value)?;
            if transport == AgentTransportKind::TerminalFallback
                && matches!(
                    support,
                    CapabilitySupport::Supported | CapabilitySupport::Conditional
                )
            {
                ensure(
                    capability == CapabilityId::TerminalIo
                        || capability == CapabilityId::SessionStatus,
                    "fallback_lacks_structure",
                )?;
            }
        }
    }
    let compatibility = fields(&map["compatibility"], &["state", "evidence", "reason"])?;
    let state = enum_value::<CompatibilityState>(&compatibility["state"])?;
    let outcomes = evidence_list(&compatibility["evidence"])?;
    reason(&compatibility["reason"])?;
    ensure(
        state != CompatibilityState::Compatible || outcomes.contains(&EvidenceOutcome::Confirmed),
        "contradictory_compatibility",
    )?;
    ensure(
        state != CompatibilityState::Unknown || !outcomes.contains(&EvidenceOutcome::Confirmed),
        "contradictory_compatibility",
    )?;
    ensure(
        state == CompatibilityState::Compatible || !compatibility["reason"].is_null(),
        "contradictory_compatibility",
    )?;
    let limitations = map["limitations"].as_array().ok_or(AdapterContractError {
        code: "expected_array",
    })?;
    ensure(limitations.len() <= 16, "limit_exceeded")?;
    for item in limitations {
        text(item, 512)?;
    }
    let extensions = object(&map["extensions"])?;
    ensure(extensions.len() <= 16, "limit_exceeded")?;
    for (key, value) in extensions {
        ensure(namespace(key), "invalid_namespace")?;
        object(value)?;
    }
    Ok(())
}
pub(super) fn model(value: &Value) -> Result {
    root(
        value,
        &[
            "schema_version",
            "provider_id",
            "model_id",
            "display_name",
            "availability",
        ],
    )?;
    let map = object(value)?;
    if !map["provider_id"].is_null() {
        id(&map["provider_id"])?;
    }
    text(&map["model_id"], 256)?;
    optional_text(&map["display_name"], 128)?;
    enum_value::<ModelAvailability>(&map["availability"])?;
    Ok(())
}
pub(super) fn session(value: &Value) -> Result {
    root(
        value,
        &[
            "schema_version",
            "managed_session_id",
            "adapter_id",
            "runtime_instance_id",
            "runtime_session_id",
            "state",
            "runtime_state",
            "observed_model",
            "terminal_id",
        ],
    )?;
    let map = object(value)?;
    uuid(&map["managed_session_id"])?;
    id(&map["adapter_id"])?;
    uuid(&map["runtime_instance_id"])?;
    optional_text(&map["runtime_session_id"], 256)?;
    enum_value::<ManagedAgentSessionState>(&map["state"])?;
    optional_text(&map["runtime_state"], 128)?;
    if !map["observed_model"].is_null() {
        model(&map["observed_model"])?;
    }
    if !map["terminal_id"].is_null() {
        uuid(&map["terminal_id"])?;
        ensure(
            !map["terminal_id"]
                .as_str()
                .unwrap()
                .eq_ignore_ascii_case(map["managed_session_id"].as_str().unwrap()),
            "identity_alias",
        )?;
    }
    Ok(())
}
pub(super) fn adapter_error(value: &Value) -> Result {
    root(
        value,
        &["schema_version", "code", "operation", "diagnostic"],
    )?;
    let map = object(value)?;
    enum_value::<AdapterErrorCode>(&map["code"])?;
    if !map["operation"].is_null() {
        enum_value::<CapabilityId>(&map["operation"])?;
    }
    if !map["diagnostic"].is_null() {
        let diagnostic = fields(&map["diagnostic"], &["code", "summary"])?;
        ensure(
            namespace(text(&diagnostic["code"], 96)?),
            "invalid_namespace",
        )?;
        text(&diagnostic["summary"], 512)?;
    }
    Ok(())
}
