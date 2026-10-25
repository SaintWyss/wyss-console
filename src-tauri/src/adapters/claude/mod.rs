//! Claude Agent SDK boundary: the pinned build, the strict codec of the supervised bridge
//! protocol and the policy that decides, from what the bridge reports, whether a runtime may
//! be used. No transport, no process and no authority live here (see `f8::claude`).
//!
//! The bridge is the one place that speaks to the official SDK. It reports; it never decides.
//! Every frame is closed (`deny_unknown_fields`), bounded and versioned, and anything the
//! policy does not recognise refuses the runtime instead of being ignored.
pub mod frames;
pub mod pins;
pub mod policy;

/// The two Managed profiles of this provider. They are `Attachment` profile strings, not wire
/// `RuntimeSafetyProfile` values: the accepted protocol-2 enumeration is Codex-only and this
/// module does not widen it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Profile {
    Inference,
    LocalTools,
}
impl Profile {
    pub const fn attachment_name(self) -> &'static str {
        match self {
            Self::Inference => "claude_inference_v1",
            Self::LocalTools => "claude_local_tools_v1",
        }
    }
    /// The name the bridge's frozen option sets use.
    pub const fn bridge_name(self) -> &'static str {
        match self {
            Self::Inference => "inference",
            Self::LocalTools => "local_tools",
        }
    }
    pub fn from_attachment(name: &str) -> Option<Self> {
        [Self::Inference, Self::LocalTools]
            .into_iter()
            .find(|p| p.attachment_name() == name)
    }
    pub fn from_bridge(name: &str) -> Option<Self> {
        [Self::Inference, Self::LocalTools]
            .into_iter()
            .find(|p| p.bridge_name() == name)
    }
}
