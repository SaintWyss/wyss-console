//! The strict, bounded codec for `StoreV1`. Input is measured before it is parsed, duplicate
//! members are refused at every depth, the schema is read before the records so a newer
//! store is reported as newer, and a decoded store is validated before it is returned. There
//! is no lenient mode and no fallback to an empty store: an input that fails is refused as it
//! is, and the caller keeps its bytes.
use super::{M1Error, M1ErrorCode, M1Result, MAX_STORE_BYTES, invalid, records::StoreV1};
use crate::contracts::control::unique_value;
use serde_json::Value;

/// Decodes and validates a store. Errors name a field and a code, never the input.
pub fn decode(bytes: &[u8]) -> M1Result<StoreV1> {
    if bytes.len() > MAX_STORE_BYTES {
        return Err(M1Error::new("store", M1ErrorCode::CapacityExceeded));
    }
    let value = unique_value(bytes).map_err(|_| invalid("store"))?;
    match value.get("schema").and_then(Value::as_u64) {
        Some(1) => {}
        // A later schema may have any shape, so it is refused before its records are read.
        Some(2..) => return Err(M1Error::new("schema", M1ErrorCode::UnsupportedSchema)),
        _ => return Err(invalid("schema")),
    }
    let store: StoreV1 = serde_json::from_value(value).map_err(|_| invalid("record"))?;
    store.validate()?;
    Ok(store)
}

/// Validates a store and writes its one canonical form: compact JSON in field order, so the
/// same store always gives the same bytes. A store past the size ceiling is refused.
pub fn encode(store: &StoreV1) -> M1Result<Vec<u8>> {
    store.validate()?;
    let bytes = serde_json::to_vec(store).map_err(|_| invalid("store"))?;
    if bytes.len() > MAX_STORE_BYTES {
        return Err(M1Error::new("store", M1ErrorCode::CapacityExceeded));
    }
    Ok(bytes)
}
