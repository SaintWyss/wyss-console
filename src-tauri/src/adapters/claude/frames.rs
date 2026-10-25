//! Strict codec of the bridge protocol (`claude-bridge/bridge.py`, version 1).
//!
//! Bridge → authority frames are one closed enum: unknown event, unknown or missing version,
//! unknown field, duplicate key, oversized frame or an out-of-bounds field is an error, never
//! something to skip. Authority → bridge frames are built from typed values only; no caller
//! supplies JSON. Counters are `Option`: a value the provider did not report is unknown, and
//! unknown never becomes zero.
use super::pins::{BRIDGE_PROTOCOL, MAX_FRAME};
use crate::contracts::control::{ControlError as E, ControlResult, unique_json};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const MAX_TEXT_BYTES: usize = 16_384;
const MAX_NAME: usize = 128;
const MAX_ITEMS: usize = 64;
/// A tool proposal's argument values together (the bridge refuses larger ones itself).
pub const MAX_ACTION_BYTES: usize = 32 * 1024;
pub const MAX_OUTPUT_BYTES: usize = 16 * 1024;
pub const MAX_PROMPT_BYTES: usize = 48 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counters {
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache_creation: Option<u64>,
    pub cache_read: Option<u64>,
    /// The provider's own count of thinking tokens (part of `output`). Turn results only.
    pub reasoning: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Named {
    pub name: Option<String>,
    pub status: Option<String>,
    pub source: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plugin {
    pub name: Option<String>,
    pub source: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEntry {
    pub value: Option<String>,
    pub resolved: Option<String>,
    pub efforts: Vec<String>,
}

/// What the bridge saw before any turn: SDK introspection that costs no provider call.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attestation {
    pub sdk_version: Option<String>,
    pub claude_code_version: Option<String>,
    pub python: Option<String>,
    pub options_digest: Option<String>,
    pub token_source: Option<String>,
    pub api_key_source: Option<String>,
    pub api_provider: Option<String>,
    pub analytics_disabled: Option<bool>,
    pub permission_mode: Option<String>,
    pub agents: Vec<String>,
    pub commands_n: u64,
    pub mcp_servers: Vec<Named>,
    pub memory_files_n: u64,
    pub mcp_tools: Vec<String>,
    pub mcp_instructions_n: u64,
    pub skills_n: Option<u64>,
    pub slash_commands_n: Option<u64>,
    pub context_categories: Vec<String>,
    pub models: Vec<ModelEntry>,
}

/// The CLI's own `system/init` inventory for a turn.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Init {
    pub tools: Vec<String>,
    pub mcp_servers: Vec<Named>,
    pub agents: Vec<String>,
    pub skills: Vec<String>,
    pub plugins: Vec<Plugin>,
    pub slash_commands_n: u64,
    pub permission_mode: Option<String>,
    pub model: Option<String>,
    pub cwd: Option<String>,
    pub api_key_source: Option<String>,
    pub claude_code_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "e", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    Hello {
        proto: u8,
        python: String,
        sdk_version: Option<String>,
    },
    Ready {
        profile: String,
        attestation: Attestation,
    },
    /// A conversation was resumed: the client was replaced and attested again.
    Bound {
        session: String,
    },
    TurnStarted {
        turn: String,
        provider_session: Option<String>,
        init: Init,
    },
    Text {
        turn: String,
        text: String,
    },
    Usage {
        turn: String,
        message_id: Option<String>,
        model: Option<String>,
        input: Option<u64>,
        output: Option<u64>,
        cache_creation: Option<u64>,
        cache_read: Option<u64>,
    },
    TurnResult {
        turn: String,
        subtype: Option<String>,
        is_error: bool,
        stop_reason: Option<String>,
        terminal_reason: Option<String>,
        api_error_status: Option<u64>,
        num_turns: Option<u64>,
        usage: Counters,
        model_usage: BTreeMap<String, Counters>,
        /// The SDK's client-side price estimate: not billing, never subscription spend.
        cost_usd_estimate: Option<f64>,
        permission_denials: u64,
        provider_session: Option<String>,
    },
    TurnIdle {
        turn: String,
    },
    ToolUse {
        turn: String,
        name: Option<String>,
    },
    ActionProposal {
        turn: String,
        action: String,
        tool: String,
        args: BTreeMap<String, String>,
    },
    Notice {
        turn: Option<String>,
        subtype: String,
    },
    /// The API's own error class for one message of the turn.
    ProviderError {
        turn: String,
        kind: String,
    },
    RateLimit {
        status: Option<String>,
        #[serde(rename = "type")]
        kind: Option<String>,
        resets_at: Option<u64>,
        utilization: Option<f64>,
    },
    TapRequest {
        path: String,
        tools: Vec<String>,
        model: Option<String>,
        effort: Option<String>,
        thinking: Option<String>,
    },
    TapViolation {
        why: String,
    },
    TapUpstreamError {},
    Error {
        turn: Option<String>,
        code: String,
    },
    Fatal {
        code: String,
    },
}

fn bad<T>() -> ControlResult<T> {
    Err(E::InvalidRequest)
}
fn name_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_NAME && !s.chars().any(char::is_control)
}
fn opt_ok(s: &Option<String>) -> bool {
    s.as_deref()
        .is_none_or(|s| s.len() <= 400 && !s.chars().any(char::is_control))
}
fn list_ok(v: &[String]) -> bool {
    v.len() <= MAX_ITEMS && v.iter().all(|s| name_ok(s))
}
fn named_ok(v: &[Named]) -> bool {
    v.len() <= 32
        && v.iter()
            .all(|n| opt_ok(&n.name) && opt_ok(&n.status) && opt_ok(&n.source))
}
fn finite(x: Option<f64>) -> bool {
    x.is_none_or(|x| x.is_finite() && x >= 0.0)
}
/// Lowercase hyphenated UUID, the one form the bridge accepts and reports.
pub fn uuid_ok(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        })
}
fn id_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_graphic())
}

impl Event {
    /// Parses one frame (without its newline). The version is checked here, so no variant
    /// carries it.
    pub fn parse(frame: &[u8]) -> ControlResult<Self> {
        if frame.len() > MAX_FRAME {
            return Err(E::FrameTooLarge);
        }
        let mut v = unique_json(frame)?;
        let obj = v.as_object_mut().ok_or(E::InvalidRequest)?;
        if obj.remove("v").and_then(|v| v.as_u64()) != Some(u64::from(BRIDGE_PROTOCOL)) {
            return Err(E::ProtocolVersionUnsupported);
        }
        let event: Self = serde_json::from_value(v).map_err(|_| E::InvalidRequest)?;
        event.validate()?;
        Ok(event)
    }

    fn validate(&self) -> ControlResult<()> {
        let ok = match self {
            Self::Hello {
                proto,
                python,
                sdk_version,
            } => *proto == BRIDGE_PROTOCOL && name_ok(python) && opt_ok(sdk_version),
            Self::Ready {
                profile,
                attestation: a,
            } => {
                name_ok(profile)
                    && opt_ok(&a.sdk_version)
                    && opt_ok(&a.claude_code_version)
                    && opt_ok(&a.python)
                    && opt_ok(&a.options_digest)
                    && opt_ok(&a.token_source)
                    && opt_ok(&a.api_key_source)
                    && opt_ok(&a.api_provider)
                    && opt_ok(&a.permission_mode)
                    && list_ok(&a.agents)
                    && named_ok(&a.mcp_servers)
                    && list_ok(&a.mcp_tools)
                    && list_ok(&a.context_categories)
                    && a.models.len() <= 32
                    && a.models.iter().all(|m| {
                        opt_ok(&m.value)
                            && opt_ok(&m.resolved)
                            && m.efforts.len() <= 16
                            && list_ok(&m.efforts)
                    })
            }
            Self::TurnStarted {
                turn,
                provider_session,
                init: i,
            } => {
                id_ok(turn)
                    && opt_ok(provider_session)
                    && list_ok(&i.tools)
                    && named_ok(&i.mcp_servers)
                    && list_ok(&i.agents)
                    && list_ok(&i.skills)
                    && i.plugins.len() <= 32
                    && i.plugins
                        .iter()
                        .all(|p| opt_ok(&p.name) && opt_ok(&p.source))
                    && opt_ok(&i.permission_mode)
                    && opt_ok(&i.model)
                    && opt_ok(&i.cwd)
                    && opt_ok(&i.api_key_source)
                    && opt_ok(&i.claude_code_version)
            }
            Self::Bound { session } => uuid_ok(session),
            Self::Text { turn, text } => {
                id_ok(turn) && !text.is_empty() && text.len() <= MAX_TEXT_BYTES
            }
            Self::Usage {
                turn,
                message_id,
                model,
                ..
            } => id_ok(turn) && opt_ok(message_id) && opt_ok(model),
            Self::TurnResult {
                turn,
                subtype,
                stop_reason,
                terminal_reason,
                model_usage,
                cost_usd_estimate,
                provider_session,
                ..
            } => {
                id_ok(turn)
                    && opt_ok(subtype)
                    && opt_ok(stop_reason)
                    && opt_ok(terminal_reason)
                    && opt_ok(provider_session)
                    && model_usage.len() <= 8
                    && model_usage.keys().all(|k| name_ok(k))
                    && finite(*cost_usd_estimate)
            }
            Self::TurnIdle { turn } => id_ok(turn),
            Self::ToolUse { turn, name } => id_ok(turn) && opt_ok(name),
            Self::ActionProposal {
                turn,
                action,
                tool,
                args,
            } => {
                id_ok(turn)
                    && id_ok(action)
                    && name_ok(tool)
                    && !args.is_empty()
                    && args.len() <= 4
                    && args.keys().all(|k| name_ok(k))
                    && args.values().map(String::len).sum::<usize>() <= MAX_ACTION_BYTES
            }
            Self::Notice { turn, subtype } => turn.as_deref().is_none_or(id_ok) && name_ok(subtype),
            Self::ProviderError { turn, kind } => id_ok(turn) && name_ok(kind),
            Self::RateLimit {
                status,
                kind,
                utilization,
                ..
            } => opt_ok(status) && opt_ok(kind) && finite(*utilization),
            Self::TapRequest {
                path,
                tools,
                model,
                effort,
                thinking,
            } => {
                name_ok(path)
                    && list_ok(tools)
                    && opt_ok(model)
                    && opt_ok(effort)
                    && opt_ok(thinking)
            }
            Self::TapViolation { why } => why.len() <= 200,
            Self::TapUpstreamError {} => true,
            Self::Error { turn, code } => turn.as_deref().is_none_or(id_ok) && name_ok(code),
            Self::Fatal { code } => name_ok(code),
        };
        if ok { Ok(()) } else { bad() }
    }
}

/// How an approved or refused action ended, as told to the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionOutcome {
    Completed,
    Denied,
    Cancelled,
    Failed,
    /// The authority cannot say whether the effect happened. The model is told not to retry.
    Indeterminate,
}
impl ActionOutcome {
    fn wire(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Denied => "denied",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Indeterminate => "indeterminate",
        }
    }
}

/// The password the bridge's tap must present to the credential broker: 32 random bytes, hex.
/// It authorizes nothing but this runtime's own route to the broker, and is never the credential.
#[derive(Clone, PartialEq, Eq)]
pub struct BrokerToken(String);
impl BrokerToken {
    pub fn new(value: String) -> ControlResult<Self> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(E::InvalidRequest);
        }
        Ok(Self(value))
    }
    pub fn generate() -> ControlResult<Self> {
        let mut raw = [0u8; 32];
        getrandom::fill(&mut raw).map_err(|_| E::StorageUnavailable)?;
        Self::new(raw.iter().map(|b| format!("{b:02x}")).collect())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Debug for BrokerToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BrokerToken(<redacted>)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Configure {
    pub profile: super::Profile,
    pub cwd: String,
    /// The conversation id the authority minted for a new conversation.
    pub session: String,
    pub resume: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// How the tap authenticates to the credential broker. The bridge is given no credential and
    /// no address of the provider: it has neither field.
    pub broker_token: BrokerToken,
    pub turn_timeout_s: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Configure(Box<Configure>),
    /// Resume an earlier conversation instead of the one `configure` named. Before any turn.
    Bind {
        resume: String,
    },
    Prompt {
        turn: String,
        text: String,
        /// A model asked for this one turn (a resolved model id).
        model: Option<String>,
    },
    Interrupt {
        turn: String,
    },
    ActionResult {
        action: String,
        outcome: ActionOutcome,
        output: String,
    },
    Shutdown,
}

impl Command {
    /// The frame, without its newline. Refuses anything the bridge would refuse.
    pub fn encode(&self) -> ControlResult<Vec<u8>> {
        let v = match self {
            Self::Configure(c) => {
                if !c.cwd.starts_with('/')
                    || !uuid_ok(&c.session)
                    || c.resume.as_deref().is_some_and(|r| !uuid_ok(r))
                    || c.cwd.len() > 1024
                    || c.cwd.chars().any(char::is_control)
                    || !(1..=3600).contains(&c.turn_timeout_s)
                {
                    return bad();
                }
                json!({"v":BRIDGE_PROTOCOL,"c":"configure","profile":c.profile.bridge_name(),
                    "cwd":c.cwd,"session":c.session,"resume":c.resume,"model":c.model,"effort":c.effort,
                    "broker_token":c.broker_token.as_str(),"turn_timeout_s":c.turn_timeout_s})
            }
            Self::Bind { resume } => {
                if !uuid_ok(resume) {
                    return bad();
                }
                json!({"v":BRIDGE_PROTOCOL,"c":"bind","resume":resume})
            }
            Self::Prompt { turn, text, model } => {
                if !id_ok(turn)
                    || text.is_empty()
                    || text.len() > MAX_PROMPT_BYTES
                    || model
                        .as_deref()
                        .is_some_and(|m| !crate::contracts::control::ModelOffer::word_ok(m, 128))
                {
                    return bad();
                }
                let mut v = json!({"v":BRIDGE_PROTOCOL,"c":"prompt","turn":turn,"text":text});
                if let Some(m) = model {
                    v["model"] = json!(m);
                }
                v
            }
            Self::Interrupt { turn } => {
                if !id_ok(turn) {
                    return bad();
                }
                json!({"v":BRIDGE_PROTOCOL,"c":"interrupt","turn":turn})
            }
            Self::ActionResult {
                action,
                outcome,
                output,
            } => {
                if !id_ok(action) || output.len() > MAX_OUTPUT_BYTES {
                    return bad();
                }
                json!({"v":BRIDGE_PROTOCOL,"c":"action_result","action":action,"outcome":outcome.wire(),"output":output})
            }
            Self::Shutdown => json!({"v":BRIDGE_PROTOCOL,"c":"shutdown"}),
        };
        let bytes = serde_json::to_vec(&v).map_err(|_| E::InvalidRequest)?;
        if bytes.len() > MAX_FRAME {
            return Err(E::FrameTooLarge);
        }
        debug_assert!(!bytes.contains(&b'\n'));
        Ok(bytes)
    }
}

/// Used by tests and the launcher's own diagnostics: whether a JSON value is a protocol frame
/// of a known kind, without interpreting it.
pub fn is_known_event_tag(v: &Value) -> bool {
    Event::parse(v.to_string().as_bytes()).is_ok()
}

#[cfg(test)]
mod tests;
