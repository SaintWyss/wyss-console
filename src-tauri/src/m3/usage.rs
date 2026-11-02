//! Provider-reported usage and limits, with provenance. The contract in
//! docs/contracts/managed-runtime.md applies here:
//!
//! - a figure the provider did not report is `None`, never zero;
//! - a figure is labelled with its provider and its grain (turn or running total);
//! - figures of different providers or sessions are never summed (a ledger belongs to one
//!   runtime generation of one provider);
//! - there is no price table: the only money figure that can appear is the one the SDK itself
//!   computed, carried as an *estimate* and never as billing or subscription spend;
//! - nothing here is durable and nothing holds prompt, output or reasoning text.
//!
//! The ledger prefers what the provider says at the end of a turn over what it said while the
//! turn was running: a streaming message's first usage report is provisional (the output count
//! is the provider's starting value) and is used only when the turn never reached a result.
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const MAX_TURNS: usize = 64;
pub const MAX_MODELS_PER_TURN: usize = 8;
pub const MAX_LIMITS: usize = 8;
const MAX_MESSAGES_PER_TURN: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Codex,
    Claude,
}

/// Token counters as one provider reported them. Every field is independently unknown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Tokens {
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache_creation: Option<u64>,
    pub cache_read: Option<u64>,
    /// The provider's own count of thinking tokens; a part of `output`, not an addition.
    pub reasoning: Option<u64>,
}
impl Tokens {
    pub fn is_unreported(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    /// The turn ended with the provider's own result: these are its final figures.
    Complete,
    /// The provider reported an error result; the figures are what it said with the error.
    ProviderError,
    /// The turn was interrupted before a result; figures come from streaming reports, which are
    /// provisional (the output count is a starting value).
    Interrupted,
    /// The turn ended with no result and no usage report at all.
    Unreported,
    /// The turn is still running.
    Running,
}

/// The SDK's own client-side price estimate. Not billing, not a subscription figure, and the
/// authority holds no price table that could have produced or checked it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SdkEstimate {
    pub usd: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TurnUsage {
    pub turn_id: String,
    pub provider: Provider,
    pub total: Tokens,
    /// Usage per model as the SDK's own bookkeeping keys it (the result's `modelUsage`, or the
    /// streaming messages when there was no result). The key is the model the *client* believes it
    /// used, so it is not evidence of which model answered.
    pub by_model: BTreeMap<String, Tokens>,
    /// The models the provider itself named in its responses to this turn (the `model` of each
    /// message). This is the "reported in use" fact. More than one is a reroute or a helper
    /// model; none is "the provider did not say".
    pub echoed_models: Vec<String>,
    pub completeness: Completeness,
    pub estimate: Option<SdkEstimate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitWindow {
    FiveHour,
    SevenDay,
    SevenDayOpus,
    SevenDaySonnet,
    Overage,
}
impl LimitWindow {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "five_hour" => Self::FiveHour,
            "seven_day" => Self::SevenDay,
            "seven_day_opus" => Self::SevenDayOpus,
            "seven_day_sonnet" => Self::SevenDaySonnet,
            "overage" => Self::Overage,
            _ => return None,
        })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitStatus {
    Allowed,
    AllowedWarning,
    Rejected,
}
impl LimitStatus {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "allowed" => Self::Allowed,
            "allowed_warning" => Self::AllowedWarning,
            "rejected" => Self::Rejected,
            _ => return None,
        })
    }
}
/// A provider-reported rate-limit window. It describes an allowance, never money.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct LimitObservation {
    pub window: LimitWindow,
    pub status: LimitStatus,
    /// Unix seconds, as the provider reported them.
    pub resets_at: Option<u64>,
    /// 0.0..=1.0 as reported.
    pub utilization: Option<f64>,
    /// When this authority received the report, in its own monotonic-free wall seconds.
    pub observed_at: u64,
}

/// One counter summed over the turns that reported it, with how many did not.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Aggregate {
    pub sum: u64,
    pub reported_turns: u32,
    pub unreported_turns: u32,
}
impl Aggregate {
    fn add(&mut self, v: Option<u64>) {
        match v {
            Some(v) => {
                self.sum = self.sum.saturating_add(v);
                self.reported_turns += 1;
            }
            None => self.unreported_turns += 1,
        }
    }
    /// A sum is only the provider's figure when every turn reported it.
    pub fn is_exact(&self) -> bool {
        self.unreported_turns == 0
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Running {
    pub input: Aggregate,
    pub output: Aggregate,
    pub cache_creation: Aggregate,
    pub cache_read: Aggregate,
    pub reasoning: Aggregate,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UsageSnapshot {
    pub provider: Provider,
    pub turns: Vec<TurnUsage>,
    /// Turns that fell out of the bounded window; their figures are in `running`, not in `turns`.
    pub dropped_turns: u64,
    pub running: Running,
    pub limits: Vec<LimitObservation>,
}

struct Open {
    turn_id: String,
    messages: BTreeMap<String, (Option<String>, Tokens)>,
    unnamed: Vec<(Option<String>, Tokens)>,
    seen: BTreeSet<String>,
    echoed: BTreeSet<String>,
}

pub struct Ledger {
    provider: Provider,
    done: VecDeque<TurnUsage>,
    dropped: u64,
    running: Running,
    open: Option<Open>,
    limits: BTreeMap<LimitWindow, LimitObservation>,
}

fn tok(t: &Tokens) -> Tokens {
    *t
}
fn add(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0).saturating_add(b.unwrap_or(0))),
    }
}

impl Ledger {
    pub fn new(provider: Provider) -> Self {
        Self {
            provider,
            done: VecDeque::new(),
            dropped: 0,
            running: Running::default(),
            open: None,
            limits: BTreeMap::new(),
        }
    }

    pub fn begin_turn(&mut self, turn_id: &str) {
        if self.open.is_some() {
            self.end_turn(Completeness::Unreported);
        }
        self.open = Some(Open {
            turn_id: turn_id.to_owned(),
            messages: BTreeMap::new(),
            unnamed: Vec::new(),
            seen: BTreeSet::new(),
            echoed: BTreeSet::new(),
        });
    }

    /// A streaming message's usage report. A repeated report of the same message id is the same
    /// message (the SDK emits one per content block) and replaces, never adds.
    pub fn message(
        &mut self,
        turn_id: &str,
        message_id: Option<&str>,
        model: Option<&str>,
        t: Tokens,
    ) {
        let Some(open) = self.open.as_mut().filter(|o| o.turn_id == turn_id) else {
            return;
        };
        if let Some(m) = model
            && open.echoed.len() < MAX_MODELS_PER_TURN
        {
            open.echoed.insert(m.to_owned());
        }
        match message_id {
            Some(id) => {
                if open.messages.len() >= MAX_MESSAGES_PER_TURN && !open.messages.contains_key(id) {
                    return;
                }
                open.seen.insert(id.to_owned());
                open.messages
                    .insert(id.to_owned(), (model.map(str::to_owned), t));
            }
            None => {
                if open.unnamed.len() < MAX_MESSAGES_PER_TURN {
                    open.unnamed.push((model.map(str::to_owned), t));
                }
            }
        }
    }

    /// The provider's result for the open turn: its final figures.
    pub fn result(
        &mut self,
        turn_id: &str,
        total: Tokens,
        by_model: BTreeMap<String, Tokens>,
        estimate: Option<f64>,
        is_error: bool,
    ) {
        let Some(open) = self.open.take_if(|o| o.turn_id == turn_id) else {
            return;
        };
        let by_model = if by_model.is_empty() {
            Self::models_from_messages(&open)
        } else {
            by_model.into_iter().take(MAX_MODELS_PER_TURN).collect()
        };
        self.finish(TurnUsage {
            turn_id: open.turn_id,
            provider: self.provider,
            total,
            by_model,
            echoed_models: open.echoed.into_iter().collect(),
            completeness: if is_error {
                Completeness::ProviderError
            } else {
                Completeness::Complete
            },
            estimate: estimate
                .filter(|e| e.is_finite() && *e >= 0.0)
                .map(|usd| SdkEstimate { usd }),
        });
    }

    /// The turn ended without a result (interrupted, failed or cut off).
    pub fn end_turn(&mut self, how: Completeness) {
        let Some(open) = self.open.take() else {
            return;
        };
        let mut total = Tokens::default();
        for (_, t) in open.messages.values().chain(open.unnamed.iter()) {
            total.input = add(total.input, t.input);
            total.output = add(total.output, t.output);
            total.cache_creation = add(total.cache_creation, t.cache_creation);
            total.cache_read = add(total.cache_read, t.cache_read);
        }
        let completeness = if total.is_unreported() {
            Completeness::Unreported
        } else {
            how
        };
        let by_model = Self::models_from_messages(&open);
        self.finish(TurnUsage {
            turn_id: open.turn_id,
            provider: self.provider,
            total,
            by_model,
            echoed_models: open.echoed.into_iter().collect(),
            completeness,
            estimate: None,
        });
    }

    fn models_from_messages(open: &Open) -> BTreeMap<String, Tokens> {
        let mut out: BTreeMap<String, Tokens> = BTreeMap::new();
        for (model, t) in open.messages.values().chain(open.unnamed.iter()) {
            let Some(model) = model else { continue };
            if out.len() >= MAX_MODELS_PER_TURN && !out.contains_key(model) {
                continue;
            }
            let e = out.entry(model.clone()).or_default();
            e.input = add(e.input, t.input);
            e.output = add(e.output, t.output);
            e.cache_creation = add(e.cache_creation, t.cache_creation);
            e.cache_read = add(e.cache_read, t.cache_read);
        }
        out
    }

    fn finish(&mut self, turn: TurnUsage) {
        self.running.input.add(turn.total.input);
        self.running.output.add(turn.total.output);
        self.running.cache_creation.add(turn.total.cache_creation);
        self.running.cache_read.add(turn.total.cache_read);
        self.running.reasoning.add(turn.total.reasoning);
        if self.done.len() == MAX_TURNS {
            self.done.pop_front();
            self.dropped += 1;
        }
        self.done.push_back(turn);
    }

    pub fn limit(&mut self, obs: LimitObservation) {
        if self.limits.len() < MAX_LIMITS || self.limits.contains_key(&obs.window) {
            self.limits.insert(obs.window, obs);
        }
    }

    /// Every model the provider named in its responses (the "reported in use" fact). A turn that
    /// has no response echo falls back to the SDK's own per-model bookkeeping, which is weaker
    /// evidence; the snapshot keeps the two apart.
    pub fn reported_models(&self) -> BTreeSet<String> {
        self.done
            .iter()
            .flat_map(|t| {
                if t.echoed_models.is_empty() {
                    t.by_model.keys().cloned().collect::<Vec<_>>()
                } else {
                    t.echoed_models.clone()
                }
            })
            .chain(self.open.iter().flat_map(|o| o.echoed.iter().cloned()))
            .collect()
    }

    pub fn snapshot(&self) -> UsageSnapshot {
        let mut turns: Vec<TurnUsage> = self.done.iter().cloned().collect();
        if let Some(open) = &self.open {
            let mut total = Tokens::default();
            for (_, t) in open.messages.values().chain(open.unnamed.iter()) {
                total.input = add(total.input, t.input);
                total.output = add(total.output, t.output);
                total.cache_creation = add(total.cache_creation, t.cache_creation);
                total.cache_read = add(total.cache_read, t.cache_read);
            }
            turns.push(TurnUsage {
                turn_id: open.turn_id.clone(),
                provider: self.provider,
                total: tok(&total),
                by_model: Self::models_from_messages(open),
                echoed_models: open.echoed.iter().cloned().collect(),
                completeness: Completeness::Running,
                estimate: None,
            });
        }
        UsageSnapshot {
            provider: self.provider,
            turns,
            dropped_turns: self.dropped,
            running: self.running,
            limits: self.limits.values().copied().collect(),
        }
    }
}

#[cfg(test)]
mod tests;
