//! Provider identity is durable data, never a credential/admission capability.
use crate::contracts::control::{ControlError as E, ControlResult};
use rusqlite::Connection;
use serde_json::Value;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Codex,
    Claude,
}
impl Provider {
    pub fn parse(adapter: &str, kind: &str) -> ControlResult<Self> {
        match (adapter, kind) {
            ("codex", "codex") => Ok(Self::Codex),
            ("claude", "claude_code") => Ok(Self::Claude),
            _ => Err(E::Unsupported),
        }
    }
    pub fn fields(self) -> (&'static str, &'static str) {
        match self {
            Self::Codex => ("codex", "codex"),
            Self::Claude => ("claude", "claude_code"),
        }
    }
    pub fn profile(self, tools: bool) -> &'static str {
        match (self, tools) {
            (Self::Codex, false) => "codex_inference_v1",
            (Self::Codex, true) => "codex_local_tools_v1",
            (Self::Claude, false) => "claude_inference_v1",
            (Self::Claude, true) => "claude_local_tools_v1",
        }
    }
}
pub fn payload(v: &Value) -> ControlResult<Provider> {
    Provider::parse(
        v["adapter_id"].as_str().ok_or(E::StorageIncompatible)?,
        v["runtime_kind"].as_str().ok_or(E::StorageIncompatible)?,
    )
}
pub fn session(c: &Connection, id: &str) -> ControlResult<Provider> {
    let (adapter, kind): (String, String) = c
        .query_row(
            "SELECT adapter_id,runtime_kind FROM managed_sessions WHERE session_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| E::SessionNotFound)?;
    Provider::parse(&adapter, &kind)
}
pub fn version(c: &Connection) -> ControlResult<i64> {
    c.pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|_| E::StorageIncompatible)
}
pub fn admit(c: &Connection, p: Provider) -> ControlResult<()> {
    match (version(c)?, p) {
        (8, Provider::Codex) | (9, _) => Ok(()),
        _ => Err(E::Unsupported),
    }
}
