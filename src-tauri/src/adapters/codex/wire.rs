use super::{CodecError, Result, fail};
use serde::{
    Deserialize, Serialize,
    de::{MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Value, json};
use std::fmt;

pub const MAX_FRAME_BYTES: usize = 262_144;
pub const MAX_CONTENT_BYTES: usize = 65_536;

macro_rules! vendor_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);
        impl $name {
            pub fn parse(value: &str) -> Result<Self> {
                text(&Value::String(value.to_owned()), 256)?;
                if value.trim().is_empty() || value.chars().any(char::is_control) {
                    return fail("invalid_vendor_id");
                }
                Ok(Self(value.to_owned()))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}
vendor_id!(CodexThreadId);
vendor_id!(CodexTurnId);
vendor_id!(CodexItemId);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum CodexRequestId {
    String(String),
    Integer(i64),
}
impl CodexRequestId {
    pub fn from_value(value: &Value) -> Result<Self> {
        match value {
            Value::String(s)
                if !s.trim().is_empty() && s.len() <= 256 && !s.chars().any(char::is_control) =>
            {
                Ok(Self::String(s.clone()))
            }
            Value::Number(n) if n.as_i64().is_some() => Ok(Self::Integer(n.as_i64().unwrap())),
            _ => fail("invalid_request_id"),
        }
    }
    pub fn validate(&self) -> Result<()> {
        Self::from_value(&serde_json::to_value(self).map_err(|_| CodecError {
            code: "invalid_request_id",
        })?)
        .map(|_| ())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    ClientToServer,
    ServerToClient,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireKind {
    ClientRequest,
    ServerRequest,
    Notification,
    SuccessResponse,
    ErrorResponse,
}

/// Validated envelope with private vendor content. Debug deliberately omits payloads.
#[derive(Clone)]
pub struct WireMessage {
    value: Value,
    kind: WireKind,
    direction: Direction,
    id: Option<CodexRequestId>,
}
impl fmt::Debug for WireMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WireMessage")
            .field("kind", &self.kind)
            .field("method", &self.method())
            .finish_non_exhaustive()
    }
}
/// Bounded structural diagnostics only: no params, result, error text or field values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct WireStructure {
    pub category_candidate: &'static str,
    pub top_level_keys: Vec<String>,
    pub keys_omitted: bool,
    pub method: Option<String>,
    pub emitted_at_ms_present: bool,
}
impl WireStructure {
    fn from_value(value: &Value) -> Self {
        let object = value.as_object();
        let has = |key| object.is_some_and(|o| o.contains_key(key));
        let category_candidate = match (has("method"), has("id"), has("result"), has("error")) {
            (true, false, false, false) => "notification",
            (true, true, false, false) => "request",
            (false, true, true, false) => "success_response",
            (false, true, false, true) => "error_response",
            _ => "unclassified",
        };
        let mut keys_omitted = false;
        let mut top_level_keys = Vec::new();
        if let Some(o) = object {
            for key in o.keys() {
                if top_level_keys.len() == 16
                    || key.len() > 64
                    || !key
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
                {
                    keys_omitted = true;
                } else {
                    top_level_keys.push(key.clone());
                }
            }
        }
        let method = value
            .get("method")
            .and_then(Value::as_str)
            .filter(|m| {
                !m.is_empty()
                    && m.len() <= 128
                    && m.bytes().all(|b| {
                        b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'.' | b'-')
                    })
            })
            .map(str::to_owned);
        Self {
            category_candidate,
            top_level_keys,
            keys_omitted,
            method,
            emitted_at_ms_present: has("emittedAtMs"),
        }
    }
}
#[derive(Debug)]
pub(crate) struct WireParseFailure {
    pub error: CodecError,
    pub structure: Option<WireStructure>,
}

impl WireMessage {
    /// One complete JSON value, optionally followed by JSON whitespace/newline.
    /// JSONL transport/framing and partial-frame accumulation are deliberately absent.
    pub fn parse(bytes: &[u8], direction: Direction) -> Result<Self> {
        Self::parse_diagnosed(bytes, direction).map_err(|failure| failure.error)
    }
    pub(crate) fn parse_diagnosed(
        bytes: &[u8],
        direction: Direction,
    ) -> std::result::Result<Self, WireParseFailure> {
        let decode = || -> Result<Value> {
            if bytes.len() > MAX_FRAME_BYTES {
                return fail("frame_too_large");
            }
            let mut decoder = serde_json::Deserializer::from_slice(bytes);
            let value = UniqueValue::deserialize(&mut decoder)
                .map_err(|_| CodecError {
                    code: "invalid_json",
                })?
                .0;
            decoder.end().map_err(|_| CodecError {
                code: "invalid_json",
            })?;
            Ok(value)
        };
        let value = decode().map_err(|error| WireParseFailure {
            error,
            structure: None,
        })?;
        let structure = WireStructure::from_value(&value);
        Self::from_value(value, direction).map_err(|error| WireParseFailure {
            error,
            structure: Some(structure),
        })
    }
    pub(crate) fn structure(&self) -> WireStructure {
        WireStructure::from_value(&self.value)
    }
    pub fn from_value(value: Value, direction: Direction) -> Result<Self> {
        let mut nodes = 0;
        bounds(&value, 0, &mut nodes)?;
        let size = serde_json::to_vec(&value)
            .map_err(|_| CodecError {
                code: "invalid_json",
            })?
            .len();
        if size > MAX_FRAME_BYTES {
            return fail("frame_too_large");
        }
        let o = object(&value)?;
        let id = o.get("id").map(CodexRequestId::from_value).transpose()?;
        let has_method = o.contains_key("method");
        let has_result = o.contains_key("result");
        let has_error = o.contains_key("error");
        let kind = match (has_method, id.is_some(), has_result, has_error) {
            (true, true, false, false) => match direction {
                Direction::ClientToServer => WireKind::ClientRequest,
                Direction::ServerToClient => WireKind::ServerRequest,
            },
            (true, false, false, false) => WireKind::Notification,
            (false, true, true, false) => WireKind::SuccessResponse,
            (false, true, false, true) => WireKind::ErrorResponse,
            _ => return fail("invalid_envelope"),
        };
        let allowed: &[&str] = match kind {
            WireKind::ClientRequest | WireKind::ServerRequest => {
                &["id", "method", "params", "trace"]
            }
            WireKind::Notification if direction == Direction::ServerToClient => {
                &["method", "params", "emittedAtMs"]
            }
            WireKind::Notification => &["method", "params"],
            WireKind::SuccessResponse => &["id", "result"],
            WireKind::ErrorResponse => &["id", "error"],
        };
        only(o, allowed)?;
        if let Some(value) = o.get("emittedAtMs").filter(|v| !v.is_null()) {
            // Official ServerNotificationEnvelope uses Option<i64>, not u64 or JS number.
            if value.as_i64().is_none() {
                return fail("invalid_emitted_at_ms");
            }
        }
        if has_method {
            let method = text(required(o, "method")?, 128)?;
            if method.is_empty()
                || !method
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'.' | b'-'))
            {
                return fail("invalid_method");
            }
        }
        if let Some(trace) = o.get("trace").filter(|v| !v.is_null()) {
            let t = object(trace)?;
            only(t, &["traceparent", "tracestate"])?;
            for v in t.values().filter(|v| !v.is_null()) {
                text(v, 1024)?;
            }
        }
        if has_error {
            let e = object(required(o, "error")?)?;
            only(e, &["code", "message", "data"])?;
            integer(required(e, "code")?)?;
            text(required(e, "message")?, 4096)?;
        }
        let message = Self {
            value,
            kind,
            direction,
            id,
        };
        super::protocol::validate_message(&message)?;
        Ok(message)
    }
    pub fn surface(&self) -> super::Surface {
        super::message_surface(self)
    }
    pub fn kind(&self) -> WireKind {
        self.kind
    }
    pub fn direction(&self) -> Direction {
        self.direction
    }
    pub fn id(&self) -> Option<&CodexRequestId> {
        self.id.as_ref()
    }
    pub fn method(&self) -> Option<&str> {
        self.value.get("method").and_then(Value::as_str)
    }
    /// Codex emission metadata only; never canonical ordering or authority.
    /// Absent/null both mean unknown; the original JSON representation is retained.
    pub fn emitted_at_ms(&self) -> Option<i64> {
        self.value.get("emittedAtMs").and_then(Value::as_i64)
    }
    pub fn params(&self) -> Option<&Value> {
        self.value.get("params")
    }
    pub fn result(&self) -> Option<&Value> {
        self.value.get("result")
    }
    /// Explicit vendor-only access; never a safe user diagnostic or common DTO.
    pub fn vendor_value(&self) -> &Value {
        &self.value
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(&self.value).map_err(|_| CodecError {
            code: "invalid_json",
        })
    }
    pub(crate) fn request(id: CodexRequestId, method: &str, params: Value) -> Result<Self> {
        id.validate()?;
        Self::from_value(
            json!({"id":id,"method":method,"params":params}),
            Direction::ClientToServer,
        )
    }
}

pub(crate) fn object(v: &Value) -> Result<&Map<String, Value>> {
    v.as_object().ok_or(CodecError {
        code: "expected_object",
    })
}
pub(crate) fn required<'a>(o: &'a Map<String, Value>, key: &str) -> Result<&'a Value> {
    o.get(key).ok_or(CodecError {
        code: "missing_field",
    })
}
pub(crate) fn text(v: &Value, max: usize) -> Result<&str> {
    match v.as_str() {
        Some(s) if s.len() <= max => Ok(s),
        _ => fail("invalid_string"),
    }
}
pub(crate) fn integer(v: &Value) -> Result<i64> {
    v.as_i64().ok_or(CodecError {
        code: "invalid_integer",
    })
}
pub(crate) fn only(o: &Map<String, Value>, keys: &[&str]) -> Result<()> {
    if o.keys().any(|k| !keys.contains(&k.as_str())) {
        fail("unknown_field")
    } else {
        Ok(())
    }
}
pub(crate) fn one_of(v: &Value, choices: &[&str]) -> Result<()> {
    if choices.contains(&text(v, 256)?) {
        Ok(())
    } else {
        fail("invalid_enum")
    }
}
pub(crate) fn array(v: &Value) -> Result<&Vec<Value>> {
    v.as_array().ok_or(CodecError {
        code: "expected_array",
    })
}
fn bounds(v: &Value, depth: usize, nodes: &mut usize) -> Result<()> {
    *nodes += 1;
    if depth > 16 || *nodes > 8192 {
        return fail("payload_bounds");
    }
    match v {
        Value::String(s) if s.len() > MAX_CONTENT_BYTES => return fail("payload_bounds"),
        Value::Array(a) => {
            if a.len() > 256 {
                return fail("payload_bounds");
            }
            for v in a {
                bounds(v, depth + 1, nodes)?;
            }
        }
        Value::Object(o) => {
            if o.len() > 256 || o.keys().any(|k| k.len() > 256) {
                return fail("payload_bounds");
            }
            for v in o.values() {
                bounds(v, depth + 1, nodes)?;
            }
        }
        _ => (),
    }
    Ok(())
}

// Reject duplicate members before serde_json::Value can overwrite them. The frame
// bound and serde's recursion limit apply before our tighter value bounds.
struct UniqueValue(Value);
impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = UniqueValue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("unambiguous JSON")
            }
            fn visit_bool<E: serde::de::Error>(
                self,
                v: bool,
            ) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| UniqueValue(Value::Number(n)))
                    .ok_or_else(|| E::custom("invalid number"))
            }
            fn visit_str<E: serde::de::Error>(
                self,
                v: &str,
            ) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_string<E: serde::de::Error>(
                self,
                v: String,
            ) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut a: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(v) = a.next_element::<UniqueValue>()? {
                    values.push(v.0);
                }
                Ok(UniqueValue(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut a: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = Map::new();
                while let Some(k) = a.next_key::<String>()? {
                    if values.contains_key(&k) {
                        return Err(serde::de::Error::custom("duplicate member"));
                    }
                    values.insert(k, a.next_value::<UniqueValue>()?.0);
                }
                Ok(UniqueValue(Value::Object(values)))
            }
        }
        d.deserialize_any(V)
    }
}
