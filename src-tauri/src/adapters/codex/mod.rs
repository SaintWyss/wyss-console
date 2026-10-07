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
