//! `ClaudeWorker`: a `Worker` that stands between the engine's normalized provider vocabulary and
//! one supervised bridge process.
//!
//! It is the Claude counterpart of `provider_adapter::NativeCodexWorker` and keeps the same
//! contract with the engine, so everything above it (identity fences, leases, one-shot
//! approvals, receipts, capacity of one) is unchanged:
//!
//! - nothing the bridge says is believed until the attestation legs pass (`adapters::claude::
//!   policy`); a failed leg makes every later call fail, it never downgrades to "warn";
//! - a model's tool call is not an effect. It becomes a held `approval_required` carrying the
//!   exact complete action, and only an accepted decision makes **this worker's executor**
//!   perform it, once; the bridge is then told how it ended;
//! - a decision is consumed before it is acted on, so an uncertain write is never repeated, and
//!   an outcome the executor cannot vouch for is told to the model as "do not retry";
//! - unknown events, wrong turns, text outside an attested turn and anything over a bound fail
//!   the worker instead of being skipped.
use super::{
    admission::Admission,
    exec::{ActionExecutor, Approved, Outcome},
    proposal::{self, Refuse},
};
use crate::{
    adapters::claude::{
        Profile,
        frames::{
            ActionOutcome, Attestation, BrokerToken, Command, Configure, Counters, Event,
            MAX_PROMPT_BYTES,
        },
        pins,
        policy::{self, NoticeClass, Refusal},
    },
    contracts::{
        agent_adapter::{AgentModelDescriptorV1, ModelProviderId},
        control::{ApprovalDecision, ControlError as E, ControlResult, ModelOffer, unique_json},
    },
    f8::{
        engine::{Attachment, Containment, StopState, Worker},
        operations::ProviderAction,
    },
    m3::{
        gate,
        history::{AsyncRecorder, Origin, Record},
        model::{self, Catalogue, ModelRequest, Requested, Scope, Source, Validated},
        report::{
            ActionRecord, ActionState, Build, OfferedModel, Phase, ProviderReport, TapSummary,
        },
        usage::{
            Completeness, Ledger, LimitObservation, LimitStatus, LimitWindow, Provider, Tokens,
        },
    },
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

/// Output the model may stream in one turn.
const MAX_TURN_TEXT: usize = 1024 * 1024;
const MAX_NORMALIZED: usize = 32;
const MAX_APPROVALS: usize = 128;
const MAX_SEEN: usize = 4096;
const MAX_TAP_LIST: usize = 16;
/// Provider retries the CLI may make within one turn before the authority ends it. Each retry is
/// another provider request; a bad credential or an outage would otherwise retry until the turn's
/// own timeout.
const MAX_API_RETRIES: u32 = 3;

/// The authority's identity of the runtime this worker serves (from its own `RuntimeRef`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub managed_session: String,
    pub runtime_instance: String,
    pub generation: u64,
}

/// Everything the spawner decided; none of it comes from a client.
pub struct ClaudeConfig {
    pub identity: Identity,
    pub profile: Profile,
    /// The attachment's workspace: the folder a local-tools runtime acts on, or the runtime's own
    /// empty folder for Inference.
    pub workspace: String,
    pub revision: u64,
    /// The provider process's working directory: an empty directory of the authority's own.
    pub cwd: String,
    /// The conversation id this runtime mints for a new binding.
    pub session: String,
    pub pinned_model: Option<String>,
    pub effort: Option<String>,
    /// The password of this runtime's route to the credential broker.
    pub broker_token: BrokerToken,
    /// The permission to exist: a production worker cannot be made while a gate is closed.
    pub admission: Admission,
    pub turn_timeout_s: u32,
    /// Test only: the engine's handshake still expects Codex's profile strings, so engine-level
    /// tests of this worker present one. Production builds have no such field.
    #[cfg(test)]
    pub attachment_profile: Option<String>,
}

struct Turn {
    managed: String,
    provider: String,
    attested: bool,
    result: bool,
    /// The user asked for the turn to stop.
    interrupted: bool,
    /// The authority ended the turn itself (retries exhausted): a failure, not a cancellation.
    cut_off: bool,
    retries: u32,
    text: usize,
    /// The model asked for this turn, validated and revalidated before dispatch.
    requested: Option<Requested>,
    /// The terminal notice, held until the bridge says it is idle: the next turn may only start
    /// once the bridge can take a prompt, so the authority must not see this turn end earlier.
    terminal: Option<Value>,
}

struct Approval {
    action: ProviderAction,
    fingerprint: String,
    approved: Approved,
    bridge_action: String,
    decision: Option<&'static str>,
    inflight: bool,
    ended: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Booting,
    Ready,
    Failed,
}

/// Where provider-reported usage may be recorded, if the owner has opted in and the policy is
/// accepted. Records go through a bounded queue to a writer thread: the event path never waits for
/// storage, so nothing a turn, a cancellation or a reap does depends on it.
pub type HistorySink = std::sync::Arc<AsyncRecorder>;

/// What has already gone to the history sink, so a figure is handed over once per worker (the
/// store deduplicates as well, across workers and restarts).
#[derive(Default)]
struct Handed {
    turns: BTreeSet<String>,
    limits: BTreeSet<(LimitWindow, u64)>,
}

pub struct ClaudeWorker<W: Worker> {
    history: Option<HistorySink>,
    handed: Handed,
    inner: W,
    cfg: ClaudeConfig,
    attachment: Attachment,
    version: String,
    stage: Stage,
    failure: Option<E>,
    refusal: Option<Refusal>,
    hello: bool,
    ready_seen: bool,
    attestation: Option<Attestation>,
    binding: Option<String>,
    pending_bind: Option<(String, String)>,
    used: bool,
    turn: Option<Turn>,
    normalized: VecDeque<Vec<u8>>,
    approvals: BTreeMap<String, Approval>,
    seen_actions: BTreeSet<String>,
    ledger: Ledger,
    exec: Box<dyn ActionExecutor>,
    inflight: Option<String>,
    tap: TapSummary,
    refused_proposals: u64,
    indeterminate: u64,
    denied_attempts: u64,
    /// Why the worker failed, in a few bounded words for diagnosis (never provider content).
    detail: Option<String>,
    /// The API's error class of the latest provider error this runtime saw.
    provider_error: Option<String>,
    /// The provider's own model listing, as last observed (the "offered" fact).
    actions: VecDeque<ActionRecord>,
    catalogue: Option<Catalogue>,
    catalogue_revision: u64,
    /// A validated model request waiting for the next turn (one-shot).
    pending_request: Option<Validated>,
    last_requested: Option<Requested>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn tokens(c: &Counters) -> Tokens {
    Tokens {
        input: c.input,
        output: c.output,
        cache_creation: c.cache_creation,
        cache_read: c.cache_read,
        reasoning: c.reasoning,
    }
}

fn push_bounded(list: &mut Vec<String>, v: &str) {
    if list.len() < MAX_TAP_LIST && !list.iter().any(|x| x == v) {
        list.push(v.to_owned());
    }
}

impl<W: Worker> ClaudeWorker<W> {
    /// The bridge process is already started (and owned) as `inner`. `configure` is written
    /// immediately; on failure the owned worker is handed back so the supervisor can reap it.
    pub fn new(
        inner: W,
        cfg: ClaudeConfig,
        executor: Box<dyn ActionExecutor>,
    ) -> Result<Self, (E, W)> {
        let mut inner = inner;
        if !cfg.admission.permits(cfg.profile) {
            return Err((E::GateDenied, inner));
        }
        let version = pins::BUILDS[0].cli_version.to_owned();
        #[cfg(test)]
        let profile_name = cfg
            .attachment_profile
            .clone()
            .unwrap_or_else(|| cfg.profile.attachment_name().to_owned());
        #[cfg(not(test))]
        let profile_name = cfg.profile.attachment_name().to_owned();
        // A model or effort chosen when the runtime starts is a selection like any other: it needs
        // the decision on ADR0054 (the fixture and tests are exempt), and an effort must be one of
        // the levels the pinned build lists. Neither is left to the word check below.
        if (cfg.pinned_model.is_some() || cfg.effort.is_some())
            && !cfg.admission.is_fixture()
            && !gate::model_selection_blockers().is_empty()
        {
            return Err((E::GateDenied, inner));
        }
        if cfg
            .effort
            .as_deref()
            .is_some_and(|e| model::validate_runtime_effort_with_gate(Provider::Claude, e).is_err())
        {
            return Err((E::InvalidRequest, inner));
        }
        if cfg.revision == 0
            || !cfg.workspace.starts_with('/')
            || cfg
                .pinned_model
                .as_deref()
                .is_some_and(|m| !ModelOffer::word_ok(m, 128))
            || cfg
                .effort
                .as_deref()
                .is_some_and(|m| !ModelOffer::word_ok(m, 32))
        {
            return Err((E::InvalidRequest, inner));
        }
        let configure = Command::Configure(Box::new(Configure {
            profile: cfg.profile,
            cwd: cfg.cwd.clone(),
            session: cfg.session.clone(),
            resume: None,
            model: cfg.pinned_model.clone(),
            effort: cfg.effort.clone(),
            broker_token: cfg.broker_token.clone(),
            turn_timeout_s: cfg.turn_timeout_s,
        }));
        let sent = configure.encode().and_then(|f| inner.send(&f));
        if let Err(e) = sent {
            return Err((e, inner));
        }
        Ok(Self {
            attachment: Attachment {
                workspace: cfg.workspace.clone(),
                profile: profile_name,
                revision: cfg.revision,
            },
            cfg,
            inner,
            version,
            stage: Stage::Booting,
            failure: None,
            refusal: None,
            hello: false,
            ready_seen: false,
            attestation: None,
            binding: None,
            pending_bind: None,
            used: false,
            turn: None,
            normalized: VecDeque::new(),
            approvals: BTreeMap::new(),
            seen_actions: BTreeSet::new(),
            ledger: Ledger::new(Provider::Claude),
            exec: executor,
            inflight: None,
            tap: TapSummary::default(),
            refused_proposals: 0,
            indeterminate: 0,
            denied_attempts: 0,
            detail: None,
            provider_error: None,
            actions: VecDeque::new(),
            catalogue: None,
            catalogue_revision: 0,
            pending_request: None,
            last_requested: None,
            history: None,
            handed: Handed::default(),
        })
    }

    /// Records finished turns' provider-reported usage and limit observations into `sink`. Off
    /// unless this is called; nothing else about the worker changes, and the sink cannot fail it.
    pub fn with_history(mut self, sink: HistorySink) -> Self {
        self.history = Some(sink);
        self
    }

    /// Hands every finished turn and limit observation not yet handed on to the history sink. A
    /// turn still running is never recorded (its figures are provisional), a figure the provider
    /// did not report stays unknown, and nothing but counters and window names leaves the ledger.
    fn record_history(&mut self) {
        let Some(sink) = self.history.clone() else {
            return;
        };
        let origin = Origin {
            provider: Provider::Claude,
            provider_build: &self.version,
            session_id: &self.cfg.identity.managed_session,
            runtime_id: &self.cfg.identity.runtime_instance,
            generation: self.cfg.identity.generation,
        };
        let snapshot = self.ledger.snapshot();
        let now = now_secs() as i64;
        for turn in snapshot
            .turns
            .iter()
            .filter(|t| t.completeness != Completeness::Running)
        {
            if self.handed.turns.insert(turn.turn_id.clone())
                && let Ok(record) = Record::turn_usage(&origin, turn, now)
            {
                sink.offer(record, now);
            }
        }
        let last_turn = snapshot.turns.last().map(|t| t.turn_id.clone());
        for limit in &snapshot.limits {
            if let Some(turn) = &last_turn
                && self.handed.limits.insert((limit.window, limit.observed_at))
                && let Ok(record) = Record::limit(&origin, turn, limit, now)
            {
                sink.offer(record, now);
            }
        }
    }

    pub fn refusal(&self) -> Option<Refusal> {
        self.refusal
    }

    fn observe_catalogue(&mut self, att: &Attestation) {
        self.catalogue_revision += 1;
        self.catalogue = Some(Catalogue {
            provider: Provider::Claude,
            build: self.version.clone(),
            revision: self.catalogue_revision,
            observed_at: now_secs(),
            complete: true,
            source: Source::ClaudeServerInfo,
            entries: offered(att)
                .into_iter()
                .map(|o| model::Entry {
                    model_id: o.model_id,
                    alias: o.alias,
                    efforts: o.efforts,
                    is_default: o.is_default,
                })
                .collect(),
        });
    }

    fn model_scope(&self) -> Scope<'_> {
        Scope {
            provider: Provider::Claude,
            build: &self.version,
            session: &self.cfg.identity.managed_session,
            runtime_instance: &self.cfg.identity.runtime_instance,
            generation: self.cfg.identity.generation,
        }
    }

    /// Asks for a model for the next turn. Refused while ADR0054 is not accepted; the request
    /// then waits for exactly one turn and is revalidated immediately before that turn is sent.
    pub fn request_model(&mut self, request: ModelRequest, now: u64) -> Result<(), model::Refusal> {
        let cat = self
            .catalogue
            .as_ref()
            .ok_or(model::Refusal::CatalogueIncomplete)?;
        let v = model::validate(&self.model_scope(), cat, now, request)?;
        self.pending_request = Some(v);
        Ok(())
    }
    /// The same admission with the gate decision taken by the caller (tests, and the handler that
    /// ADR0054's acceptance will add).
    #[allow(dead_code)]
    pub(crate) fn request_model_with_gate(
        &mut self,
        request: ModelRequest,
        now: u64,
    ) -> Result<(), model::Refusal> {
        let cat = self
            .catalogue
            .as_ref()
            .ok_or(model::Refusal::CatalogueIncomplete)?;
        let v = model::validate_with_gate(&self.model_scope(), cat, now, request)?;
        self.pending_request = Some(v);
        Ok(())
    }

    fn log_action(
        &mut self,
        item: &str,
        tool: &str,
        state: ActionState,
        refusal: Option<&'static str>,
    ) {
        if let Some(a) = self.actions.iter_mut().find(|a| a.item == item) {
            a.state = state;
            if refusal.is_some() {
                a.refusal = refusal;
            }
            return;
        }
        if self.actions.len() >= 32 {
            self.actions.pop_front();
        }
        self.actions.push_back(ActionRecord {
            item: item.to_owned(),
            tool: tool.to_owned(),
            state,
            refusal,
        });
    }

    fn fail_with<T>(&mut self, e: E, detail: String) -> ControlResult<T> {
        let mut d = detail;
        d.truncate(120);
        self.detail.get_or_insert(d);
        self.fail(e)
    }
    /// Actions still waiting for a decision when their turn (or the runtime) ends are withdrawn, and
    /// the action log says so: found in the integrated desktop, where an interrupted turn kept
    /// showing its command as "waiting for your decision" with live buttons.
    fn withdraw_undecided(&mut self) {
        let items: Vec<String> = self
            .approvals
            .iter()
            .filter(|(_, a)| a.decision.is_none() && !a.inflight && !a.ended)
            .map(|(item, _)| item.clone())
            .collect();
        for item in items {
            self.log_action(&item, "", ActionState::Cancelled, None);
        }
    }
    fn fail<T>(&mut self, e: E) -> ControlResult<T> {
        self.stage = Stage::Failed;
        self.failure.get_or_insert(e);
        // Nothing held survives a failure: the provider can no longer be told anything.
        self.withdraw_undecided();
        self.approvals.clear();
        Err(e)
    }
    fn refuse<T>(&mut self, r: Refusal) -> ControlResult<T> {
        self.refusal.get_or_insert(r);
        self.fail(E::GateDenied)
    }
    fn usable(&self) -> ControlResult<()> {
        match (self.stage, self.failure) {
            (_, Some(e)) => Err(e),
            (Stage::Ready, None) => Ok(()),
            _ => Err(E::InvalidTransition),
        }
    }

    fn send_cmd(&mut self, c: &Command) -> ControlResult<()> {
        let frame = c.encode()?;
        self.inner.send(&frame)
    }

    fn notice(&mut self, v: Value) -> ControlResult<()> {
        if self.normalized.len() >= MAX_NORMALIZED {
            return self.fail(E::CapacityExceeded);
        }
        self.normalized
            .push_back(serde_json::to_vec(&v).map_err(|_| E::InternalError)?);
        Ok(())
    }

    // ---- models ------------------------------------------------------------------------
    fn models_notice(&self) -> ControlResult<Value> {
        let att = self.attestation.as_ref().ok_or(E::InvalidTransition)?;
        let mut models: Vec<AgentModelDescriptorV1> = Vec::new();
        let mut offers: Vec<ModelOffer> = Vec::new();
        for o in offered(att) {
            let provider = ModelProviderId::parse("anthropic").ok();
            let d = AgentModelDescriptorV1::from_value(
                json!({"schema_version":1,"provider_id":provider,
                "model_id":o.model_id,"display_name":o.alias,"availability":"unknown"}),
            )
            .map_err(|_| E::InvalidRequest)?;
            let offer = ModelOffer {
                is_default: o.is_default,
                default_effort: None,
                efforts: o.efforts,
                replaced_by: None,
                retires_at: None,
            };
            if d.validate().is_err() || !offer.valid() {
                return Err(E::InvalidRequest);
            }
            models.push(d);
            offers.push(offer);
        }
        Ok(json!({"kind":"models","models":models,"offers":offers,"complete":true}))
    }

    // ---- bridge events -----------------------------------------------------------------
    fn turn_for(&mut self, id: &str) -> ControlResult<&mut Turn> {
        match self.turn.as_mut() {
            Some(t) if t.managed == id => Ok(t),
            _ => Err(E::CorrelationError),
        }
    }

    fn on_event(&mut self, ev: Event) -> ControlResult<()> {
        let outcome = self.on_event_inner(ev);
        self.record_history();
        outcome
    }

    fn on_event_inner(&mut self, ev: Event) -> ControlResult<()> {
        match ev {
            Event::Hello { .. } => {
                if std::mem::replace(&mut self.hello, true) {
                    return self.fail(E::InvalidTransition);
                }
                Ok(())
            }
            Event::Ready {
                profile,
                attestation,
            } => {
                if !self.hello {
                    return self.fail(E::InvalidTransition);
                }
                if let Err(r) = policy::verify_ready(self.cfg.profile, &profile, &attestation) {
                    return self.refuse(r);
                }
                self.observe_catalogue(&attestation);
                if !self.ready_seen {
                    self.ready_seen = true;
                    self.attestation = Some(attestation);
                    self.stage = Stage::Ready;
                    let n = json!({"kind":"runtime_ready","observed_version":self.version,
                        "workspace":self.attachment.workspace,"profile":self.attachment.profile,
                        "revision":self.attachment.revision});
                    self.notice(n)?;
                    let m = self.models_notice()?;
                    self.notice(m)
                } else if self.pending_bind.is_some() {
                    // The client was replaced to resume a conversation: attested again.
                    self.attestation = Some(attestation);
                    Ok(())
                } else {
                    self.fail(E::InvalidTransition)
                }
            }
            Event::Bound { session } => {
                let Some((operation, resume)) = self.pending_bind.take() else {
                    return self.fail(E::InvalidTransition);
                };
                if session != resume {
                    return self.fail(E::CorrelationError);
                }
                self.binding = Some(session.clone());
                self.notice(json!({"kind":"binding_confirmed","operation_id":operation,"provider_thread_id":session}))
            }
            Event::TurnStarted {
                turn,
                provider_session,
                init,
            } => {
                let binding = self.binding.clone();
                let t = self.turn_for(&turn)?;
                if t.attested {
                    return self.fail(E::InvalidTransition);
                }
                // The conversation the provider reports must be the one Wyss bound.
                if provider_session.is_some() && provider_session != binding {
                    return self.fail(E::CorrelationError);
                }
                if let Err(r) = policy::verify_turn_started(self.cfg.profile, &init) {
                    return self.refuse(r);
                }
                let t = self.turn_for(&turn)?;
                t.attested = true;
                let provider = t.provider.clone();
                self.notice(json!({"kind":"turn_running","managed_turn_id":turn,"provider_turn_id":provider}))
            }
            Event::Text { turn, text } => {
                let t = self.turn_for(&turn)?;
                if !t.attested {
                    return self.fail(E::GateDenied);
                }
                t.text += text.len();
                if t.text > MAX_TURN_TEXT {
                    return self.fail(E::CapacityExceeded);
                }
                let provider = t.provider.clone();
                self.notice(json!({"kind":"content","managed_turn_id":turn,"provider_turn_id":provider,"text":text}))
            }
            Event::Usage {
                turn,
                message_id,
                model,
                input,
                output,
                cache_creation,
                cache_read,
            } => {
                self.turn_for(&turn)?;
                self.ledger.message(
                    &turn,
                    message_id.as_deref(),
                    model.as_deref(),
                    Tokens {
                        input,
                        output,
                        cache_creation,
                        cache_read,
                        reasoning: None,
                    },
                );
                Ok(())
            }
            Event::ToolUse { turn, name } => {
                let attested = self.turn_for(&turn)?.attested;
                let ours = self.cfg.profile == Profile::LocalTools
                    && name
                        .as_deref()
                        .is_some_and(|n| policy::AUTHORITY_TOOLS.contains(&n));
                if !attested || !ours {
                    // The model asked for something that was never offered: the API list and the
                    // CLI disagree, which is exactly what this check exists to notice.
                    return self.refuse(Refusal::ExecutableToolSurface);
                }
                Ok(())
            }
            Event::ActionProposal {
                turn,
                action,
                tool,
                args,
            } => self.on_proposal(&turn, action, &tool, args),
            Event::TurnResult {
                turn,
                subtype,
                is_error,
                terminal_reason,
                usage,
                model_usage,
                cost_usd_estimate,
                ..
            } => {
                let (interrupted, cut_off) = {
                    let t = self.turn_for(&turn)?;
                    (t.interrupted, t.cut_off)
                };
                let by_model: BTreeMap<String, Tokens> = model_usage
                    .iter()
                    .map(|(k, v)| (k.clone(), tokens(v)))
                    .collect();
                let failed = is_error || subtype.as_deref() != Some("success");
                self.ledger
                    .result(&turn, tokens(&usage), by_model, cost_usd_estimate, failed);
                let aborted = terminal_reason
                    .as_deref()
                    .is_some_and(|r| r.starts_with("aborted"));
                let t = self.turn.as_mut().ok_or(E::CorrelationError)?;
                t.result = true;
                let provider = t.provider.clone();
                // What ended the turn decides its name: a stop the user asked for is an interruption
                // even though the CLI reports it as an error result; a turn the authority or the
                // CLI cut off is a failure; an abort nobody asked for is a failure too.
                let notice = if interrupted && !cut_off && (aborted || failed) {
                    json!({"kind":"turn_interrupted","managed_turn_id":turn})
                } else if failed || aborted || cut_off {
                    json!({"kind":"turn_failed","managed_turn_id":turn})
                } else {
                    json!({"kind":"turn_completed","managed_turn_id":turn,"provider_turn_id":provider})
                };
                self.turn.as_mut().ok_or(E::CorrelationError)?.terminal = Some(notice);
                Ok(())
            }
            Event::TurnIdle { turn } => {
                let t = self.turn_for(&turn)?;
                let had_result = t.result;
                let interrupted = t.interrupted;
                let terminal = t.terminal.take();
                self.turn = None;
                // Held approvals belong to a turn that no longer exists; an in-flight dispatch
                // keeps running to its own end and is reported when it finishes.
                self.withdraw_undecided();
                self.approvals.retain(|_, a| a.inflight);
                if let Some(notice) = terminal {
                    return self.notice(notice);
                }
                if !had_result {
                    self.ledger.end_turn(if interrupted {
                        Completeness::Interrupted
                    } else {
                        Completeness::Unreported
                    });
                    return self.notice(json!({"kind":"turn_failed","managed_turn_id":turn}));
                }
                Ok(())
            }
            Event::Notice { turn, subtype } => {
                if let Some(t) = &turn {
                    self.turn_for(t)?;
                }
                match policy::classify_notice(&subtype) {
                    NoticeClass::Benign => {
                        if subtype == "api_retry" {
                            self.count_retry(turn.as_deref())?;
                        }
                        if subtype == "permission_denied" {
                            self.denied_attempts += 1;
                        }
                        Ok(())
                    }
                    // The requested model was replaced and nobody authorized that.
                    NoticeClass::Substitution => self.refuse(Refusal::ModelSubstituted),
                    // An event this build was never qualified against.
                    NoticeClass::Unknown => {
                        self.fail_with(E::InvalidRequest, format!("notice:{subtype}"))
                    }
                }
            }
            Event::ProviderError { turn, kind } => {
                self.turn_for(&turn)?;
                if !policy::PROVIDER_ERRORS.contains(&kind.as_str()) {
                    return self.fail_with(E::InvalidRequest, format!("provider_error:{kind}"));
                }
                // The turn's own result follows and ends it as a failure; this names the cause.
                self.provider_error = Some(kind);
                Ok(())
            }
            Event::RateLimit {
                status,
                kind,
                resets_at,
                utilization,
            } => {
                if let (Some(w), Some(s)) = (
                    kind.as_deref().and_then(LimitWindow::parse),
                    status.as_deref().and_then(LimitStatus::parse),
                ) {
                    self.ledger.limit(LimitObservation {
                        window: w,
                        status: s,
                        resets_at,
                        utilization,
                        observed_at: now_secs(),
                    });
                }
                Ok(())
            }
            Event::TapRequest {
                path,
                tools,
                model,
                effort,
                thinking,
            } => {
                // The model the provider must receive: the one asked for this turn, else the one the
                // runtime was configured with, else none is pinned.
                let expected = self
                    .turn
                    .as_ref()
                    .and_then(|t| t.requested.as_ref().map(|r| r.resolved_model.clone()))
                    .or_else(|| self.cfg.pinned_model.clone());
                if let Err(r) = policy::verify_tap(
                    self.cfg.profile,
                    &path,
                    &tools,
                    model.as_deref(),
                    expected.as_deref(),
                ) {
                    self.tap.violations += 1;
                    return self.refuse(r);
                }
                self.tap.requests += 1;
                for t in &tools {
                    push_bounded(&mut self.tap.tools_seen, t);
                }
                if path == "/v1/messages" {
                    if let Some(m) = &model {
                        push_bounded(&mut self.tap.models_seen, m);
                    }
                    if let Some(e) = &effort {
                        push_bounded(&mut self.tap.efforts_seen, e);
                    }
                }
                let _ = thinking;
                Ok(())
            }
            Event::TapViolation { .. } => {
                self.tap.violations += 1;
                self.refuse(Refusal::ProviderRequestOffersTool)
            }
            Event::TapUpstreamError {} => Ok(()),
            Event::Error { turn, code } => {
                if code == "unknown_action" {
                    return self.fail_with(E::InvalidTransition, "error:unknown_action".into());
                }
                if code.starts_with("turn_timeout") || code == "turn_active" {
                    return Ok(());
                }
                if code.starts_with("sdk_error:") && turn.is_some() {
                    // The turn's own `turn_idle` follows and closes it.
                    return Ok(());
                }
                self.fail_with(E::ProviderRejected, format!("error:{code}"))
            }
            Event::Fatal { code } => self.fail_with(E::ProviderRejected, format!("fatal:{code}")),
        }
    }

    fn count_retry(&mut self, turn: Option<&str>) -> ControlResult<()> {
        let Some(turn) = turn else {
            return Ok(());
        };
        let over = {
            let t = self.turn_for(turn)?;
            t.retries += 1;
            t.retries > MAX_API_RETRIES && !t.cut_off
        };
        if over {
            self.turn_for(turn)?.cut_off = true;
            // Best effort: if the bridge cannot be told, the turn's own timeout ends it.
            let _ = self.send_cmd(&Command::Interrupt {
                turn: turn.to_owned(),
            });
        }
        Ok(())
    }

    fn on_proposal(
        &mut self,
        turn: &str,
        action: String,
        tool: &str,
        args: BTreeMap<String, String>,
    ) -> ControlResult<()> {
        let attested = self.turn_for(turn)?.attested;
        if self.cfg.profile != Profile::LocalTools || !attested {
            return self.refuse(Refusal::ExecutableToolSurface);
        }
        if self.seen_actions.len() >= MAX_SEEN || !self.seen_actions.insert(action.clone()) {
            return self.fail(E::IntentConflict);
        }
        // One held action at a time, as the authority admits one pending approval.
        if self
            .approvals
            .values()
            .any(|a| a.decision.is_none() || a.inflight)
            || self.approvals.len() >= MAX_APPROVALS
        {
            return self.fail(E::IntentConflict);
        }
        let item = format!("claude-{action}");
        match proposal::prepare(&self.attachment.workspace, tool, &args, &item) {
            Err(why) => {
                self.log_action(&item, tool, ActionState::Refused, Some(why.code()));
                self.refuse_proposal(&action, why)
            }
            Ok(p) => {
                self.log_action(&item, tool, ActionState::Held, None);
                let (kind, fingerprint, structure) = p.action.metadata(&item)?;
                let event = json!({"kind":"approval_required","approval_kind":kind,"provider_item_id":item,
                    "action_fingerprint":fingerprint,"structure":structure,"action":p.action});
                self.approvals.insert(
                    item,
                    Approval {
                        action: p.action,
                        fingerprint,
                        approved: p.approved,
                        bridge_action: action,
                        decision: None,
                        inflight: false,
                        ended: false,
                    },
                );
                self.notice(event)
            }
        }
    }

    /// The proposal is not put to the user; the model is told why, in words that carry no path
    /// or content of the host.
    fn refuse_proposal(&mut self, action: &str, why: Refuse) -> ControlResult<()> {
        self.refused_proposals += 1;
        self.send_cmd(&Command::ActionResult {
            action: action.to_owned(),
            outcome: ActionOutcome::Failed,
            output: why.model_text().to_owned(),
        })
    }

    // ---- executor ----------------------------------------------------------------------
    fn poll_executor(&mut self, now: Instant) -> ControlResult<()> {
        let Some(item) = self.inflight.clone() else {
            // Nothing is running, but the namespace of an action that just ended is still being
            // stopped: it is reaped here, or the executor stays busy and no turn is admitted.
            if self.exec.busy() {
                let _ = self.exec.poll_stop(now);
            }
            return Ok(());
        };
        let Some(outcome) = self.exec.poll(now) else {
            return Ok(());
        };
        self.inflight = None;
        let Some(a) = self.approvals.get_mut(&item) else {
            return Ok(());
        };
        a.inflight = false;
        a.ended = true;
        let bridge_action = a.bridge_action.clone();
        let (result, confirmed, output) = match outcome {
            Outcome::Completed(text) => (ActionOutcome::Completed, true, text),
            Outcome::Failed(text) => (ActionOutcome::Failed, false, text),
            Outcome::Indeterminate => {
                self.indeterminate += 1;
                (ActionOutcome::Indeterminate, false, String::new())
            }
        };
        let state = match result {
            ActionOutcome::Completed => ActionState::Completed,
            ActionOutcome::Failed => ActionState::Failed,
            _ => ActionState::Indeterminate,
        };
        self.log_action(&item, "", state, None);
        if self.turn.is_none() {
            self.approvals.remove(&item);
        }
        // The bridge may already be gone (the turn ended or was interrupted): the effect's
        // outcome is still reported to the authority below.
        let _ = self.send_cmd(&Command::ActionResult {
            action: bridge_action,
            outcome: result,
            output,
        });
        if confirmed {
            self.notice(json!({"kind":"approval_item_confirmed","provider_item_id":item}))?;
        }
        Ok(())
    }

    /// `approval <item> accept|decline|cancel` from the engine. The decision is consumed first.
    fn decide(&mut self, frame: &[u8]) -> ControlResult<()> {
        self.usable()?;
        if self.cfg.profile != Profile::LocalTools {
            return Err(E::Unsupported);
        }
        let text = std::str::from_utf8(frame).map_err(|_| E::InvalidRequest)?;
        let words: Vec<_> = text.split(' ').collect();
        if words.len() != 3 || words[0] != "approval" {
            return Err(E::InvalidRequest);
        }
        let (item, decision) = (words[1].to_owned(), words[2]);
        let workspace = self.attachment.workspace.clone();
        let a = self.approvals.get_mut(&item).ok_or(E::ApprovalNotFound)?;
        if a.decision.is_some() || a.ended || a.action.metadata(&item)?.1 != a.fingerprint {
            return Err(E::CorrelationError);
        }
        let decision: &'static str = match decision {
            "accept" => "accept",
            "decline" => "decline",
            "cancel" => "cancel",
            _ => return Err(E::InvalidRequest),
        };
        a.decision = Some(decision);
        let bridge_action = a.bridge_action.clone();
        if decision != "accept" {
            a.ended = true;
            let outcome = if decision == "decline" {
                ActionOutcome::Denied
            } else {
                ActionOutcome::Cancelled
            };
            self.log_action(
                &item,
                "",
                if decision == "decline" {
                    ActionState::Declined
                } else {
                    ActionState::Cancelled
                },
                None,
            );
            let sent = self.send_cmd(&Command::ActionResult {
                action: bridge_action,
                outcome,
                output: String::new(),
            });
            self.notice(json!({"kind":"approval_item_consumed","provider_item_id":item}))?;
            return sent;
        }
        // The proof is repeated at the moment of the effect.
        let still = a
            .action
            .clone()
            .build(&item)
            .map(|h| h.confined(&workspace))
            .unwrap_or(false);
        if !still || self.turn.is_none() {
            a.ended = true;
            let _ = self.send_cmd(&Command::ActionResult {
                action: bridge_action,
                outcome: ActionOutcome::Failed,
                output: Refuse::OutOfScope.model_text().to_owned(),
            });
            return Err(E::GateDenied);
        }
        let approved: Approved = a.approved.clone();
        a.inflight = true;
        match self.exec.dispatch(approved) {
            Ok(()) => {
                self.log_action(&item, "", ActionState::Running, None);
                self.inflight = Some(item.clone());
                self.notice(json!({"kind":"approval_item_consumed","provider_item_id":item}))
            }
            Err(e) => {
                if let Some(a) = self.approvals.get_mut(&item) {
                    a.inflight = false;
                    a.ended = true;
                }
                // Nothing was started, so the effect did not happen.
                let _ = self.send_cmd(&Command::ActionResult {
                    action: bridge_action,
                    outcome: ActionOutcome::Failed,
                    output: "the authority could not start this action; nothing was run".into(),
                });
                Err(e)
            }
        }
    }

    // ---- engine frames -----------------------------------------------------------------
    fn outbound(&mut self, frame: &[u8]) -> ControlResult<()> {
        self.usable()?;
        let v = unique_json(frame)?;
        match v["method"].as_str() {
            Some("turn/start") => {
                if self.turn.is_some() || self.pending_bind.is_some() || self.exec.busy() {
                    return Err(E::ActiveTurnExists);
                }
                if self.binding.is_none() {
                    return Err(E::BindingNotFound);
                }
                let managed = v["turn"].as_str().ok_or(E::InvalidRequest)?.to_owned();
                let input = v["input"].as_str().ok_or(E::InvalidRequest)?.to_owned();
                if input.len() > MAX_PROMPT_BYTES {
                    return Err(E::FrameTooLarge);
                }
                // A model request is checked once more at the moment of dispatch: nothing it was
                // validated for may have changed (generation, runtime, catalogue, freshness).
                let requested = match self.pending_request.take() {
                    None => None,
                    Some(v) => {
                        let ok = self.catalogue.as_ref().is_some_and(|c| {
                            model::revalidate(&v, &self.model_scope(), c, now_secs()).is_ok()
                        });
                        if !ok {
                            return Err(E::GateDenied);
                        }
                        Some(Requested {
                            turn_id: managed.clone(),
                            request: v.request,
                            resolved_model: v.resolved_model,
                            generation: v.bound.generation,
                        })
                    }
                };
                let cmd = Command::Prompt {
                    turn: managed.clone(),
                    text: input,
                    model: requested.as_ref().map(|r| r.resolved_model.clone()),
                };
                // The turn exists from the moment the prompt may have reached the bridge.
                self.turn = Some(Turn {
                    provider: format!("claude-{managed}"),
                    managed: managed.clone(),
                    attested: false,
                    result: false,
                    interrupted: false,
                    cut_off: false,
                    retries: 0,
                    text: 0,
                    requested: requested.clone(),
                    terminal: None,
                });
                self.last_requested = requested;
                self.used = true;
                self.ledger.begin_turn(&managed);
                self.send_cmd(&cmd)
            }
            Some("turn/interrupt") => {
                let t = self.turn.as_mut().ok_or(E::InvalidTransition)?;
                t.interrupted = true;
                let turn = t.managed.clone();
                self.send_cmd(&Command::Interrupt { turn })
            }
            Some(m @ ("thread/start" | "thread/resume")) => {
                if self.binding.is_some() || self.pending_bind.is_some() || self.turn.is_some() {
                    return Err(E::BindingAlreadyExists);
                }
                let operation = v["operation_id"]
                    .as_str()
                    .ok_or(E::InvalidRequest)?
                    .to_owned();
                if v["workspace"].as_str() != Some(self.attachment.workspace.as_str()) {
                    return Err(E::GateDenied);
                }
                if m == "thread/start" {
                    let s = self.cfg.session.clone();
                    self.binding = Some(s.clone());
                    return self.notice(json!({"kind":"binding_confirmed","operation_id":operation,"provider_thread_id":s}));
                }
                let id = v["thread_id"].as_str().ok_or(E::InvalidRequest)?.to_owned();
                if id == self.cfg.session {
                    self.binding = Some(id.clone());
                    return self.notice(json!({"kind":"binding_confirmed","operation_id":operation,"provider_thread_id":id}));
                }
                if self.used {
                    return Err(E::InvalidTransition);
                }
                self.send_cmd(&Command::Bind { resume: id.clone() })?;
                self.pending_bind = Some((operation, id));
                Ok(())
            }
            Some("model/list") => {
                let n = self.models_notice()?;
                self.notice(n)
            }
            _ => Err(E::Unsupported),
        }
    }

    fn drain(&mut self, max: usize) -> Vec<Vec<u8>> {
        self.normalized
            .drain(..max.min(self.normalized.len()))
            .collect()
    }
}

/// The models the provider's own catalogue offers, one entry per resolved model. The alias
/// `default` resolves to a model that is also offered by its own name: it marks that one as the
/// default instead of listing it twice. Offered is not entitlement.
pub fn offered(att: &Attestation) -> Vec<OfferedModel> {
    let mut out: Vec<OfferedModel> = Vec::new();
    let default_target = att
        .models
        .iter()
        .find(|m| m.value.as_deref() == Some("default"))
        .and_then(|m| m.resolved.clone());
    for m in &att.models {
        let (Some(value), Some(resolved)) = (&m.value, &m.resolved) else {
            continue;
        };
        if value == "default" {
            continue;
        }
        if out.iter().any(|o| &o.model_id == resolved) {
            continue;
        }
        out.push(OfferedModel {
            model_id: resolved.clone(),
            alias: Some(value.clone()),
            is_default: default_target.as_deref() == Some(resolved.as_str()),
            efforts: m.efforts.clone(),
        });
    }
    out
}

impl<W: Worker> Worker for ClaudeWorker<W> {
    fn approval_decisions(&self, item: &str) -> Vec<ApprovalDecision> {
        match self.approvals.get(item) {
            Some(a) if a.decision.is_none() && !a.ended => {
                vec![
                    ApprovalDecision::Accept,
                    ApprovalDecision::Decline,
                    ApprovalDecision::Cancel,
                ]
            }
            _ => Vec::new(),
        }
    }
    fn preflight(&mut self, context: &Attachment) -> ControlResult<()> {
        if context != &self.attachment {
            return Err(E::GateDenied);
        }
        self.usable()?;
        if self.cfg.profile == Profile::LocalTools
            && self.inner.containment() != Containment::PidNamespace
        {
            return Err(E::GateDenied);
        }
        Ok(())
    }
    fn send(&mut self, frame: &[u8]) -> ControlResult<()> {
        self.outbound(frame)
    }
    fn send_critical(&mut self, frame: &[u8]) -> ControlResult<()> {
        self.decide(frame)
    }
    fn flush(&mut self) -> ControlResult<()> {
        self.inner.flush()
    }
    fn inbound(&mut self, max_frames: usize) -> ControlResult<Vec<Vec<u8>>> {
        if max_frames == 0 {
            return Ok(Vec::new());
        }
        if !self.normalized.is_empty() {
            return Ok(self.drain(max_frames));
        }
        if let Some(e) = self.failure {
            return Err(e);
        }
        self.poll_executor(Instant::now())?;
        let frames = self.inner.inbound(max_frames.min(16))?;
        for frame in frames {
            let ev = match Event::parse(&frame) {
                Ok(ev) => ev,
                Err(e) => return self.fail_with(e, format!("frame:{e:?}")),
            };
            self.on_event(ev)?;
        }
        Ok(self.drain(max_frames))
    }
    fn has_exited(&mut self) -> bool {
        self.inner.has_exited()
    }
    fn begin_stop(&mut self, now: Instant) {
        self.stage = Stage::Failed;
        self.failure.get_or_insert(E::InvalidTransition);
        self.withdraw_undecided();
        self.approvals.clear();
        if self.turn.take().is_some() {
            self.ledger.end_turn(Completeness::Interrupted);
            self.record_history();
        }
        self.exec.begin_stop(now);
        self.inner.begin_stop(now);
    }
    fn poll_stop(&mut self, now: Instant) -> StopState {
        let exec = self.exec.poll_stop(now);
        let inner = self.inner.poll_stop(now);
        match (exec, inner) {
            (StopState::Reaped, StopState::Reaped) => StopState::Reaped,
            (StopState::Stuck, _) | (_, StopState::Stuck) => StopState::Stuck,
            _ => StopState::Pending,
        }
    }
    fn descendants(&self) -> usize {
        self.inner.descendants()
    }
    fn containment(&mut self) -> Containment {
        self.inner.containment()
    }
    fn pid_for_diagnostics(&self) -> Option<i32> {
        self.inner.pid_for_diagnostics()
    }
    fn provider_report(&self) -> Option<ProviderReport> {
        let att = self.attestation.as_ref();
        Some(ProviderReport {
            provider: Provider::Claude,
            profile: self.cfg.profile.attachment_name().to_owned(),
            phase: match (self.stage, self.refusal) {
                (Stage::Ready, None) => Phase::Ready,
                (Stage::Booting, None) => Phase::Starting,
                _ => Phase::Refused,
            },
            refusal: self.refusal.map(Refusal::code),
            build: Build {
                sdk_version: att.and_then(|a| a.sdk_version.clone()),
                cli_version: att.and_then(|a| a.claude_code_version.clone()),
            },
            offered: att.map(offered).unwrap_or_default(),
            tap: self.tap.clone(),
            usage: self.ledger.snapshot(),
            refused_proposals: self.refused_proposals,
            pending_approvals: self
                .approvals
                .values()
                .filter(|a| a.decision.is_none())
                .count() as u64,
            indeterminate_actions: self.indeterminate,
            denied_attempts: self.denied_attempts,
            failure: self.detail.clone(),
            actions: self.actions.iter().cloned().collect(),
            provider_error: self.provider_error.clone(),
            model: Some(model::facts(
                self.catalogue.as_ref(),
                now_secs(),
                self.cfg
                    .pinned_model
                    .as_deref()
                    .and_then(|m| ModelRequest::new(m, self.cfg.effort.as_deref()).ok())
                    .or_else(|| {
                        self.cfg
                            .effort
                            .as_deref()
                            .and_then(|e| ModelRequest::new("provider-default", Some(e)).ok())
                    }),
                self.last_requested.clone(),
                &self.ledger.reported_models(),
                self.tap.efforts_seen.last().map(String::as_str),
                None,
            )),
        })
    }
}

#[cfg(test)]
mod tests;
