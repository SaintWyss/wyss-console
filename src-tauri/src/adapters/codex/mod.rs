//! Codex codec and an explicitly invoked read-only developer probe. No Tauri API.
//! Vendor IDs are not Wyss identities.
//!
//! ```compile_fail
//! use wyss_console::{adapters::codex::CodexThreadId, contracts::identities::ManagedSessionId};
//! let thread = CodexThreadId::parse("thread-1").unwrap();
//! let session: ManagedSessionId = thread;
//! ```
mod normalize;
#[cfg(unix)]
pub mod probe;
mod protocol;
pub(crate) mod safety;
mod state;
mod wire;
pub use normalize::*;
pub use protocol::*;
pub use state::*;
pub use wire::*;
#[cfg(test)]
mod tests;

/// Whether managed Inference could start on a configuration made of these definitions, as
/// the authority decides it: the same two checks (`zero_external_hooks` on the hook
/// definitions of each source, `mcp_names` on the server names), run on a response built from
/// the definitions. It exists so that the desktop's prediction of a configuration change
/// (F2b, TS-CFG-15) can be held equal to the authority's rule by a test; it starts nothing,
/// reads nothing and decides nothing at run time. `None` is "no blocker".
pub fn managed_inference_blocker(
    hook_sources: &[Vec<bool>],
    mcp_names: &[String],
) -> Option<&'static str> {
    use serde_json::json;
    // `enabled` is what `hooks/list` reports: true when the hook is on.
    let sources: Vec<_> = hook_sources
        .iter()
        .map(|hooks| json!({ "hooks": hooks.iter().map(|on| json!({ "enabled": on })).collect::<Vec<_>>() }))
        .collect();
    if let Err(error) = safety::zero_external_hooks(&json!({ "data": sources })) {
        return Some(error.code);
    }
    let servers: serde_json::Map<String, serde_json::Value> = mcp_names
        .iter()
        .map(|name| (name.clone(), json!({ "enabled": false })))
        .collect();
    if let Err(error) = safety::mcp_names(&json!({ "config": { "mcp_servers": servers } }), true) {
        return Some(error.code);
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodecError {
    pub code: &'static str,
}
impl std::fmt::Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code)
    }
}
impl std::error::Error for CodecError {}
type Result<T> = std::result::Result<T, CodecError>;
fn fail<T>(code: &'static str) -> Result<T> {
    Err(CodecError { code })
}
