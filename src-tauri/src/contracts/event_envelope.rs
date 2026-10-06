use super::{
    ContractError, ErrorCode, Sequence, UtcTimestamp, WireUuid, identities::ManagedSessionId,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

const FIELDS: [&str; 15] = [
    "schema_version",
    "event_id",
    "event_type",
    "payload_schema_version",
    "timestamp",
    "stream_id",
    "sequence",
    "session_id",
    "source",
    "correlation_id",
    "causation_id",
    "payload",
    "persistence",
    "sensitivity",
    "redaction",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Persistence {
    Durable,
    #[default]
    Ephemeral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    Public,
    Internal,
    Sensitive,
    Secret,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Redaction {
    None,
    Applied,
    ContentOmitted,
}

#[derive(Debug, Clone, Serialize)]
pub struct Source {
    component: String,
    instance_id: WireUuid,
}

/// A validated wire contract, independent of IPC, streams, persistence and authority.
/// Private fields prevent construction/mutation which bypasses the entrypoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "Value")]
pub struct EventEnvelopeV1 {
    schema_version: u8,
    event_id: WireUuid,
    event_type: String,
    payload_schema_version: Number,
    timestamp: UtcTimestamp,
    stream_id: WireUuid,
    sequence: Sequence,
    session_id: Option<ManagedSessionId>,
    source: Source,
    correlation_id: Option<WireUuid>,
    causation_id: Option<WireUuid>,
    payload: Map<String, Value>,
    persistence: Persistence,
    sensitivity: Sensitivity,
    redaction: Redaction,
}

impl EventEnvelopeV1 {
    pub fn from_value(value: Value) -> Result<Self, ContractError> {
        let object = as_object(&value, "$")?;
        fields(object, &FIELDS, "$")?;
        if object["schema_version"].as_f64() != Some(1.0) {
            return Err(ContractError::new(
                "schema_version",
                ErrorCode::InvalidSchemaVersion,
            ));
        }
        let event_id = uuid(&object["event_id"], "event_id")?;
        let event_type = text(
            &object["event_type"],
            "event_type",
            ErrorCode::InvalidEventType,
        )?;
        if !event_type.contains('.') || !event_type.split('.').all(valid_name_segment) {
            return Err(ContractError::new(
                "event_type",
                ErrorCode::InvalidEventType,
            ));
        }
        let payload_schema_version = object["payload_schema_version"]
            .as_number()
            .filter(|number| {
                number
                    .as_f64()
                    .is_some_and(|n| n.is_finite() && n > 0.0 && n.fract() == 0.0)
            })
            .ok_or_else(|| {
                ContractError::new("payload_schema_version", ErrorCode::InvalidPositiveInteger)
            })?
            .clone();
        let timestamp = UtcTimestamp::try_from(
            text(
                &object["timestamp"],
                "timestamp",
                ErrorCode::InvalidTimestamp,
            )?
            .to_owned(),
        )?;
        let stream_id = uuid(&object["stream_id"], "stream_id")?;
        let sequence =
            text(&object["sequence"], "sequence", ErrorCode::InvalidSequence)?.parse()?;
        let session_id = optional_uuid(&object["session_id"], "session_id")?
            .map(|id| ManagedSessionId::parse(id.as_str()))
            .transpose()?;
        let source_object = as_object(&object["source"], "source")?;
        fields(source_object, &["component", "instance_id"], "source")?;
        let component = text(
            &source_object["component"],
            "source.component",
            ErrorCode::InvalidComponent,
        )?;
        if component.trim().is_empty() || component.chars().any(char::is_control) {
            return Err(ContractError::new(
                "source.component",
                ErrorCode::InvalidComponent,
            ));
        }
        let source = Source {
            component: component.to_owned(),
            instance_id: uuid(&source_object["instance_id"], "source.instance_id")?,
        };
        let correlation_id = optional_uuid(&object["correlation_id"], "correlation_id")?;
        let causation_id = optional_uuid(&object["causation_id"], "causation_id")?;
        let payload = as_object(&object["payload"], "payload")?.clone();
        let persistence = enum_value::<Persistence>(&object["persistence"], "persistence")?;
        let sensitivity = enum_value::<Sensitivity>(&object["sensitivity"], "sensitivity")?;
        let redaction = enum_value::<Redaction>(&object["redaction"], "redaction")?;
        if persistence == Persistence::Durable && sensitivity == Sensitivity::Secret {
            return Err(ContractError::new("sensitivity", ErrorCode::DurableSecret));
        }
        Ok(Self {
            schema_version: 1,
            event_id,
            event_type: event_type.to_owned(),
            payload_schema_version,
            timestamp,
            stream_id,
            sequence,
            session_id,
            source,
            correlation_id,
            causation_id,
            payload,
            persistence,
            sensitivity,
            redaction,
        })
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        Self::from_value(
            serde_json::to_value(self)
                .map_err(|_| ContractError::new("$", ErrorCode::InvalidJsonValue))?,
        )
        .map(|_| ())
    }

    pub const fn sequence(&self) -> Sequence {
        self.sequence
    }

    pub const fn persistence(&self) -> Persistence {
        self.persistence
    }
}

impl TryFrom<Value> for EventEnvelopeV1 {
    type Error = ContractError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        Self::from_value(value)
    }
}

fn as_object<'a>(
    value: &'a Value,
    field: &'static str,
) -> Result<&'a Map<String, Value>, ContractError> {
    value
        .as_object()
        .ok_or_else(|| ContractError::new(field, ErrorCode::ExpectedObject))
}

fn fields(
    object: &Map<String, Value>,
    required: &[&'static str],
    field: &'static str,
) -> Result<(), ContractError> {
    for name in required {
        if !object.contains_key(*name) {
            let path = match (field, *name) {
                ("source", "component") => "source.component",
                ("source", "instance_id") => "source.instance_id",
                _ => *name,
            };
            return Err(ContractError::new(path, ErrorCode::MissingField));
        }
    }
    if object.keys().any(|name| !required.contains(&name.as_str())) {
        return Err(ContractError::new(field, ErrorCode::UnexpectedField));
    }
    Ok(())
}

fn text<'a>(
    value: &'a Value,
    field: &'static str,
    code: ErrorCode,
) -> Result<&'a str, ContractError> {
    value
        .as_str()
        .ok_or_else(|| ContractError::new(field, code))
}

fn uuid(value: &Value, field: &'static str) -> Result<WireUuid, ContractError> {
    WireUuid::try_from(text(value, field, ErrorCode::InvalidUuid)?.to_owned())
        .map_err(|_| ContractError::new(field, ErrorCode::InvalidUuid))
}

fn optional_uuid(value: &Value, field: &'static str) -> Result<Option<WireUuid>, ContractError> {
    if value.is_null() {
        Ok(None)
    } else {
        uuid(value, field).map(Some)
    }
}

fn enum_value<T: serde::de::DeserializeOwned>(
    value: &Value,
    field: &'static str,
) -> Result<T, ContractError> {
    serde_json::from_value(value.clone())
        .map_err(|_| ContractError::new(field, ErrorCode::InvalidEnum))
}

fn valid_name_segment(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}
