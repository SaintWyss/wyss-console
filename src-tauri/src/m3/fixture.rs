//! A conversation with a fixture Claude runtime, for the window.
//!
//! `FixtureModel` is the pure part: it folds the worker's normalized notices into what the window
//! shows (the conversation, the held actions, the turn). `FixtureHost` (only with the
//! `m3-fixture` feature) is the thin shell that owns a real `ClaudeWorker`, the real SDK and a mock
//! provider API and drives them from a thread. It is **not the authority**: nothing it does is
//! durable, leased or fenced, and it says so on every payload.
use super::{
    gui::{ApprovalView, FixtureView, Message, TurnView},
    report::{ActionState, ProviderReport},
};
use serde_json::Value;
use std::collections::BTreeMap;

pub const LABEL: &str = "FIXTURE HOST — hosted by this window, not the authority";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reaction {
    None,
    /// The runtime is ready and listed its models: the host binds a conversation.
    Bind,
}

struct Held {
    kind: String,
    fingerprint: String,
    action: Value,
}

pub struct FixtureModel {
    phase: &'static str,
    profile: Option<&'static str>,
    conversation: Vec<Message>,
    turn: Option<TurnView>,
    held: BTreeMap<String, Held>,
    order: Vec<String>,
    detail: Option<String>,
    ready_seen: bool,
    assistant_turn: Option<String>,
}

const MAX_MESSAGES: usize = 200;
const MAX_HELD: usize = 16;

impl Default for FixtureModel {
    fn default() -> Self {
        Self::new()
    }
}
impl FixtureModel {
    pub fn new() -> Self {
        Self {
            phase: "stopped",
            profile: None,
            conversation: Vec::new(),
            turn: None,
            held: BTreeMap::new(),
            order: Vec::new(),
            detail: None,
            ready_seen: false,
            assistant_turn: None,
        }
    }
    pub fn starting(&mut self, profile: &'static str) {
        *self = Self::new();
        self.phase = "starting";
        self.profile = Some(profile);
    }
    pub fn stopped(&mut self) {
        self.phase = "stopped";
        self.turn = None;
        self.held.clear();
    }
    pub fn failed(&mut self, detail: &str) {
        self.phase = "failed";
        let mut d = detail.to_owned();
        d.truncate(160);
        self.detail = Some(d);
        self.held.clear();
    }
    pub fn refused(&mut self, detail: &str) {
        self.failed(detail);
        self.phase = "refused";
    }
    pub fn phase(&self) -> &'static str {
        self.phase
    }

    fn push(&mut self, role: &'static str, text: String) {
        if self.conversation.len() >= MAX_MESSAGES {
            self.conversation.remove(0);
        }
        self.conversation.push(Message { role, text });
    }

    pub fn prompt(&mut self, turn_id: &str, text: &str) {
        self.push("user", text.to_owned());
        self.turn = Some(TurnView {
            id: turn_id.to_owned(),
            state: "starting",
        });
        self.assistant_turn = None;
    }

    pub fn apply(&mut self, notice: &Value) -> Reaction {
        match notice["kind"].as_str() {
            Some("runtime_ready") => {
                self.ready_seen = true;
            }
            Some("models") if self.ready_seen && self.phase == "starting" => return Reaction::Bind,
            Some("binding_confirmed") => self.phase = "ready",
            Some("turn_running") => {
                if let Some(t) = &mut self.turn {
                    t.state = "running";
                }
            }
            Some("content") => {
                let turn = notice["managed_turn_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let text = notice["text"].as_str().unwrap_or_default();
                if self.assistant_turn.as_deref() == Some(turn.as_str())
                    && let Some(last) = self.conversation.last_mut()
                    && last.role == "assistant"
                {
                    last.text.push_str(text);
                } else {
                    self.push("assistant", text.to_owned());
                    self.assistant_turn = Some(turn);
                }
            }
            Some(k @ ("turn_completed" | "turn_failed" | "turn_interrupted")) => {
                if let Some(t) = &mut self.turn {
                    t.state = match k {
                        "turn_completed" => "completed",
                        "turn_failed" => "failed",
                        _ => "interrupted",
                    };
                }
            }
            Some("approval_required") => {
                let item = notice["provider_item_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                if !item.is_empty() && self.held.len() < MAX_HELD && !self.held.contains_key(&item)
                {
                    self.order.push(item.clone());
                    self.held.insert(
                        item,
                        Held {
                            kind: notice["approval_kind"]
                                .as_str()
                                .unwrap_or("unknown")
                                .to_owned(),
                            fingerprint: notice["action_fingerprint"]
                                .as_str()
                                .unwrap_or_default()
                                .to_owned(),
                            action: notice["action"].clone(),
                        },
                    );
                }
            }
            _ => (),
        }
        Reaction::None
    }

    pub fn view(&self, report: Option<ProviderReport>) -> FixtureView {
        let state_of = |item: &str| -> &'static str {
            report
                .as_ref()
                .and_then(|r| r.actions.iter().find(|a| a.item == item))
                .map_or("pending", |a| match a.state {
                    ActionState::Held => "pending",
                    ActionState::Declined => "declined",
                    ActionState::Cancelled => "cancelled",
                    ActionState::Running => "accepted",
                    ActionState::Completed => "confirmed",
                    ActionState::Failed => "failed",
                    ActionState::Indeterminate => "indeterminate",
                    ActionState::Refused => "refused",
                })
        };
        let approvals = self
            .order
            .iter()
            .filter_map(|item| self.held.get(item).map(|h| (item, h)))
            .map(|(item, h)| {
                let state = state_of(item);
                ApprovalView {
                    item: item.clone(),
                    kind: h.kind.clone(),
                    fingerprint: h.fingerprint.clone(),
                    action: h.action.clone(),
                    decisions: if state == "pending" && self.phase == "ready" {
                        vec!["accept", "decline", "cancel"]
                    } else {
                        vec![]
                    },
                    state,
                }
            })
            .collect();
        FixtureView {
            label: LABEL,
            phase: self.phase,
            profile: self.profile,
            conversation: self.conversation.clone(),
            turn: self.turn.clone(),
            approvals,
            report,
            detail: self.detail.clone(),
        }
    }
}

#[cfg(feature = "m3-fixture")]
pub use host::{FixtureHost, FixtureOptions, MARKER, extension};

#[cfg(feature = "m3-fixture")]
pub mod host;

#[cfg(test)]
mod tests;
