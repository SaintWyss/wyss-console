//! Whether a configuration entry may hold a secret (11 §5.4, TS-SEC-07).
//!
//! The answer decides where an entry may be kept (ADR 0034): an entry that may hold a secret
//! is kept only sealed and is not kept at all when the key store is unavailable. So the
//! check is conservative on purpose: doubt counts as "may hold a secret", and a wrong
//! answer in that direction costs a refused plan, never a leaked value.
//!
//! It runs on the parsed entry through the same redaction that protects the page, so what
//! the page would be shown with a mask is exactly what makes an entry secret-bearing.
use super::model::Bounds;
use super::redact::project;
use serde_json::Value;

/// Names whose presence alone makes a handler secret-bearing, whatever the values: the
/// shapes in which a provider's hook carries credentials.
const CREDENTIAL_SLOTS: [&str; 6] = [
    "env",
    "headers",
    "http_headers",
    "allowedEnvVars",
    "bearer_token_env_var",
    "authorization",
];

pub fn may_hold_secret(entry: &Value, bounds: &Bounds) -> bool {
    // An entry deeper than the bounds cannot be judged, and is not trusted.
    let Ok(projection) = project(entry, bounds) else {
        return true;
    };
    projection.may_hold_secret() || names_a_credential_slot(entry)
}

fn names_a_credential_slot(value: &Value) -> bool {
    match value {
        Value::Object(entries) => entries.iter().any(|(key, inner)| {
            CREDENTIAL_SLOTS
                .iter()
                .any(|slot| key.eq_ignore_ascii_case(slot))
                && !is_empty(inner)
                || names_a_credential_slot(inner)
        }),
        Value::Array(items) => items.iter().any(names_a_credential_slot),
        _ => false,
    }
}

fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Object(entries) => entries.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::String(text) => text.is_empty(),
        _ => false,
    }
}
