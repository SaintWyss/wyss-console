//! Shared 2C transport/handshake, retained under the authority after catalog readiness.
use super::*;
use crate::adapters::codex::safety;
use crate::contracts::{
    UtcTimestamp, WireUuid,
    control::{RuntimeDiagnostic, RuntimeMethods},
    identities::AgentRuntimeInstanceId,
};
use std::collections::BTreeSet;
use std::{
    io::Write,
    os::unix::net::UnixStream,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
};

pub(crate) enum RuntimeCommand {
    Conversation(ConversationCommand),
    Start(TurnCommand),
    Interrupt {
        managed_turn_id: crate::contracts::identities::ManagedTurnId,
        thread_id: CodexThreadId,
        turn_id: CodexTurnId,
    },
}
pub(crate) struct TurnCommand {
    pub managed_turn_id: crate::contracts::identities::ManagedTurnId,
    pub thread_id: CodexThreadId,
    pub workspace: String,
    pub prompt: String,
    pub expires_at: UtcTimestamp,
    pub admission: Arc<AtomicBool>,
}
pub(crate) struct ConversationCommand {
    pub operation_id: WireUuid,
    pub request_id: CodexRequestId,
    pub workspace: String,
    pub thread_id: Option<CodexThreadId>,
    pub expires_at: UtcTimestamp,
    pub admission: Arc<AtomicBool>,
}
#[derive(Debug, Clone, Copy)]
pub(crate) enum ConversationFailure {
    NotDispatched,
    Rejected,
    Unknown,
}
#[derive(Debug, Clone, Copy)]
pub(crate) enum MaterializationStage {
    Started,
    Archived,
    Unarchived,
}
pub(crate) enum Observation {
    Materialization {
        operation_id: WireUuid,
        thread_id: CodexThreadId,
        stage: MaterializationStage,
        reply: SyncSender<bool>,
    },
    Conversation {
        operation_id: WireUuid,
        result: std::result::Result<CodexThreadId, ConversationFailure>,
        methods: RuntimeMethods,
    },
    TurnResult {
        managed_turn_id: crate::contracts::identities::ManagedTurnId,
        result: std::result::Result<CodexTurnId, ConversationFailure>,
        methods: RuntimeMethods,
    },
    InterruptResult {
        managed_turn_id: crate::contracts::identities::ManagedTurnId,
        success: bool,
        methods: RuntimeMethods,
    },
    IsolationViolation,
    TurnNotice(safety::TurnNotice),
    SafetyConfigChanged(BTreeSet<String>),
    Discovering,
    Spawned,
    Ready {
        version: String,
        models: Vec<AgentModelDescriptorV1>,
        methods: RuntimeMethods,
    },
    Exited {
        reason: RuntimeDiagnostic,
        methods: RuntimeMethods,
    },
}
/// How one generation is launched. A generation never changes profile.
pub(crate) enum LaunchProfile {
    Inference,
    LocalTools(isolation::Isolation),
}
/// Authority-fixed launch inputs of one runtime generation.
struct Generation {
    forced_mcp: BTreeSet<String>,
    catalog_dir: std::path::PathBuf,
    profile: LaunchProfile,
}
/// Facts of an isolated generation, fixed when its namespace is created.
struct Local<'a> {
    isolation: &'a isolation::Isolation,
    /// Project policy layers masked for the lifetime of the namespace.
    masked: Vec<std::path::PathBuf>,
    /// Roots forced untrusted on every thread.
    roots: Vec<String>,
    home: String,
    workspace: String,
}
impl Local<'_> {
    fn spawn(&self, executable: &Path, args: Vec<String>) -> ProbeResult<Command> {
        self.isolation.command(executable, args, &self.masked)
    }
}
pub(crate) struct Worker {
    cancel: Arc<AtomicBool>,
    wake: UnixStream,
    thread: Option<JoinHandle<RuntimeMethods>>,
    commands: SyncSender<RuntimeCommand>,
}
impl Worker {
    /// Fixed discovery, argv and cwd. No client-provided process inputs.
    pub fn start(
        instance: AgentRuntimeInstanceId,
        forced_mcp: BTreeSet<String>,
        catalog_dir: std::path::PathBuf,
        profile: LaunchProfile,
        mut observe: impl FnMut(&AgentRuntimeInstanceId, Observation, &AtomicBool) + Send + 'static,
    ) -> std::io::Result<Self> {
        let (wake, reader) = UnixStream::pair()?;
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let (commands, rx) = mpsc::sync_channel(1);
        let generation = Generation {
            forced_mcp,
            catalog_dir,
            profile,
        };
        let thread = thread::Builder::new()
            .name("codex-runtime".into())
            .spawn(move || {
                let mut methods = RuntimeMethods::default();
                let result = run(
                    &instance,
                    &flag,
                    reader,
                    rx,
                    &generation,
                    &mut observe,
                    &mut methods,
                );
                if !flag.load(Ordering::Acquire) {
                    observe(
                        &instance,
                        Observation::Exited {
                            reason: result.err().unwrap_or(RuntimeDiagnostic::ChildExited),
                            methods: methods.clone(),
                        },
                        &flag,
                    );
                }
                methods
            })?;
        Ok(Self {
            cancel,
            wake,
            thread: Some(thread),
            commands,
        })
    }
    pub fn command(&mut self, command: RuntimeCommand) -> bool {
        if self.commands.try_send(command).is_err() {
            return false;
        }
        self.wake.write_all(&[1]).is_ok()
    }
    pub fn stop(&mut self) -> Option<RuntimeMethods> {
        self.cancel.store(true, Ordering::Release);
        let _ = self.wake.write_all(&[1]);
        self.thread.take().and_then(|thread| thread.join().ok())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
fn diagnostic(error: ProbeError) -> RuntimeDiagnostic {
    match error {
        ProbeError::InferenceIsolationViolation => RuntimeDiagnostic::InferenceIsolationViolation,
        ProbeError::RuntimeUnavailable => RuntimeDiagnostic::Unavailable,
        ProbeError::Spawn => RuntimeDiagnostic::SpawnFailed,
        ProbeError::Transport | ProbeError::Shutdown => RuntimeDiagnostic::TransportFailed,
        ProbeError::Timeout => RuntimeDiagnostic::HandshakeTimeout,
        ProbeError::RuntimeRejected => RuntimeDiagnostic::RuntimeRejected,
        ProbeError::UnexpectedServerRequest => RuntimeDiagnostic::ServerRequest,
        ProbeError::UnexpectedEof => RuntimeDiagnostic::ChildExited,
        _ => RuntimeDiagnostic::ProtocolFailed,
    }
}
/// Counts the actual successful sends at the IO boundary. Rejects responses and all
/// other methods before writing any byte; independent fake-child tests inspect stdin.
struct RuntimeIo<'a> {
    stdio: Stdio<'a>,
    methods: &'a mut RuntimeMethods,
    discovery: bool,
    forward: Option<&'a mut dyn FnMut(Observation)>,
    active_pair: Option<(CodexThreadId, CodexTurnId)>,
    isolation_violation: bool,
    /// Authority model catalog of this generation. Discovery has none and starts no thread.
    catalog: Option<&'a Path>,
    /// Present on an isolated local-tools process, including its discovery.
    local: Option<&'a Local<'a>>,
    /// The process itself reported the private Codex home in its initialize response.
    home_proven: bool,
}
impl RuntimeIo<'_> {
    fn isolation_failure(&mut self) -> ProbeError {
        self.isolation_violation = true;
        if let Some((thread_id, turn_id)) = self.active_pair.clone()
            && let Ok(request) = (ClientOperation::TurnInterrupt { thread_id, turn_id })
                .encode(CodexRequestId::String(uuid::Uuid::new_v4().to_string()))
            && let Ok(mut bytes) = request.encode()
        {
            bytes.push(b'\n');
            let _ = self.send(&bytes, Instant::now() + Duration::from_millis(250));
        }
        self.emit(Observation::IsolationViolation);
        ProbeError::InferenceIsolationViolation
    }
    fn emit(&mut self, event: Observation) {
        if let Some(f) = self.forward.as_mut() {
            f(event);
        }
    }
}
impl ProbeIo for RuntimeIo<'_> {
    fn send(&mut self, bytes: &[u8], deadline: Instant) -> ProbeResult<()> {
        let message = WireMessage::parse(
            bytes.strip_suffix(b"\n").ok_or(ProbeError::Codec)?,
            Direction::ClientToServer,
        )
        .map_err(|_| ProbeError::Codec)?;
        if !message.method().is_some_and(|m| {
            if self.local.is_some() {
                safety::LOCAL_TOOLS_METHODS.contains(&m)
            } else {
                safety::PRODUCTION_METHODS.contains(&m)
            }
        }) {
            return Err(ProbeError::Protocol);
        }
        if message.method() == Some("configRequirements/read") {
            if self.discovery
                || message
                    .params()
                    .and_then(serde_json::Value::as_object)
                    .is_none_or(|p| !p.is_empty())
            {
                return Err(ProbeError::Protocol);
            }
            self.stdio.send(bytes, deadline)?;
            self.methods.config_requirements_read =
                self.methods.config_requirements_read.saturating_add(1);
            return Ok(());
        }
        if self.discovery
            && !matches!(
                message.method(),
                Some("initialize" | "initialized" | "model/list" | "config/read")
            )
        {
            return Err(ProbeError::Protocol);
        }
        if matches!(message.method(), Some("turn/start" | "turn/interrupt")) {
            let p = message
                .params()
                .and_then(serde_json::Value::as_object)
                .ok_or(ProbeError::Protocol)?;
            if p.len() != 2
                || !p
                    .get("threadId")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|s| CodexThreadId::parse(s).is_ok())
            {
                return Err(ProbeError::Protocol);
            }
            if message.method() == Some("turn/start") {
                let input = p
                    .get("input")
                    .and_then(serde_json::Value::as_array)
                    .ok_or(ProbeError::Protocol)?;
                if input.len() != 1
                    || input[0].as_object().is_none_or(|o| o.len() != 3)
                    || input[0]["type"] != "text"
                    || input[0]["textElements"] != serde_json::json!([])
                    || input[0]["text"]
                        .as_str()
                        .is_none_or(|s| crate::contracts::turn::validate_prompt(s).is_err())
                {
                    return Err(ProbeError::Protocol);
                }
            } else if !p
                .get("turnId")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|s| CodexTurnId::parse(s).is_ok())
            {
                return Err(ProbeError::Protocol);
            }
            self.stdio.send(bytes, deadline)?;
            let count = if message.method() == Some("turn/start") {
                &mut self.methods.turn_start
            } else {
                &mut self.methods.turn_interrupt
            };
            *count = count.saturating_add(1);
            return Ok(());
        }
        if matches!(message.method(), Some("config/read" | "hooks/list")) {
            let p = message
                .params()
                .and_then(serde_json::Value::as_object)
                .ok_or(ProbeError::Protocol)?;
            let valid = if message.method() == Some("config/read") {
                p.len() == 2
                    && p.get("includeLayers") == Some(&serde_json::json!(false))
                    && p.get("cwd")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|v| Path::new(v).is_absolute())
            } else {
                p.len() == 1
                    && p.get("cwds")
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|a| {
                            a.len() == 1
                                && a[0].as_str().is_some_and(|v| Path::new(v).is_absolute())
                        })
            };
            if !valid {
                return Err(ProbeError::Protocol);
            }
            self.stdio.send(bytes, deadline)?;
            let count = if message.method() == Some("config/read") {
                &mut self.methods.config_read
            } else {
                &mut self.methods.hooks_list
            };
            *count = count.saturating_add(1);
            return Ok(());
        }
        if matches!(
            message.method(),
            Some("thread/archive" | "thread/unarchive")
        ) {
            let v = message.vendor_value();
            let p = v["params"].as_object().ok_or(ProbeError::Protocol)?;
            if p.len() != 1
                || !p
                    .get("threadId")
                    .and_then(|v| v.as_str())
                    .is_some_and(|id| CodexThreadId::parse(id).is_ok())
            {
                return Err(ProbeError::Protocol);
            }
            self.stdio.send(bytes, deadline)?;
            let count = if message.method() == Some("thread/archive") {
                &mut self.methods.thread_archive
            } else {
                &mut self.methods.thread_unarchive
            };
            *count = count.saturating_add(1);
            return Ok(());
        }
        if matches!(message.method(), Some("thread/start" | "thread/resume")) {
            let v = message.vendor_value();
            let p = v["params"].as_object().ok_or(ProbeError::Protocol)?;
            let start = message.method() == Some("thread/start");
            // The posture of a thread is fixed by the generation's profile, never by a caller.
            let (sandbox, policy) = if self.local.is_some() {
                ("workspace-write", "untrusted")
            } else {
                ("read-only", "never")
            };
            let posture = p.get("sandbox") == Some(&serde_json::json!(sandbox))
                && p.get("approvalPolicy") == Some(&serde_json::json!(policy))
                && p.get("config").is_some_and(serde_json::Value::is_object);
            let valid = if start && self.local.is_none() {
                p.len() == 2 && p.get("ephemeral") == Some(&serde_json::json!(false))
            } else if start {
                p.len() == 5 && posture && p.get("ephemeral") == Some(&serde_json::json!(false))
            } else {
                p.len() == 6
                    && posture
                    && p.get("excludeTurns") == Some(&serde_json::json!(true))
                    && p.get("threadId")
                        .and_then(|v| v.as_str())
                        .is_some_and(|id| CodexThreadId::parse(id).is_ok())
            };
            if self
                .local
                .is_some_and(|l| p.get("cwd").and_then(|v| v.as_str()) != Some(&l.workspace))
            {
                return Err(ProbeError::Protocol);
            }
            if !valid
                || !p
                    .get("cwd")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| Path::new(s).is_absolute())
            {
                return Err(ProbeError::Protocol);
            }
            self.stdio.send(bytes, deadline)?;
            if message.method() == Some("thread/start") {
                self.methods.thread_start = self.methods.thread_start.saturating_add(1)
            } else {
                self.methods.thread_resume = self.methods.thread_resume.saturating_add(1)
            }
            return Ok(());
        }
        let counter = match message.method() {
            Some("initialize") => &mut self.methods.initialize,
            Some("initialized") => &mut self.methods.initialized,
            Some("model/list") => &mut self.methods.model_list,
            _ => return Err(ProbeError::Protocol),
        };
        if *counter >= 3 {
            return Err(ProbeError::Protocol);
        }
        self.stdio.send(bytes, deadline)?;
        *counter += 1;
        Ok(())
    }
    fn receive(&mut self, deadline: Instant) -> ProbeResult<Option<WireMessage>> {
        let message = match self.stdio.receive(deadline) {
            Err(_)
                if self.stdio.rejected.as_ref().is_some_and(|s| {
                    s.category_candidate == "request" || s.external_action_candidate
                }) =>
            {
                return Err(self.isolation_failure());
            }
            result => result?,
        };
        if let (Some(m), Some(local)) = (&message, self.local)
            && m.kind() == WireKind::SuccessResponse
            && let Some(home) = m.result().and_then(|r| r.get("codexHome"))
        {
            if home.as_str() != Some(&local.home) {
                return Err(ProbeError::Protocol);
            }
            self.home_proven = true;
        }
        if let Some(m) = &message {
            let notice = match safety::notification(m) {
                Ok(n) => n,
                Err(e) => {
                    if e.code == "inference_isolation_violation" {
                        return Err(self.isolation_failure());
                    }
                    return Err(ProbeError::Protocol);
                }
            };
            if notice.as_ref().is_some_and(|n| {
                matches!(n.kind, safety::NoticeKind::Terminal(_))
                    && self
                        .active_pair
                        .as_ref()
                        .is_some_and(|(thread, turn)| thread == &n.thread && turn == &n.turn)
            }) {
                self.active_pair = None;
            }
            if let (Some(notice), Some(forward)) = (notice, self.forward.as_mut()) {
                forward(Observation::TurnNotice(notice));
            }
        }
        Ok(message)
    }
    fn close_input(&mut self) {
        self.stdio.close_input();
    }
}
fn run(
    instance: &AgentRuntimeInstanceId,
    cancel: &Arc<AtomicBool>,
    wake: UnixStream,
    commands: Receiver<RuntimeCommand>,
    generation: &Generation,
    observe: &mut impl FnMut(&AgentRuntimeInstanceId, Observation, &AtomicBool),
    methods: &mut RuntimeMethods,
) -> std::result::Result<(), RuntimeDiagnostic> {
    let cwd = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let executable =
        discover(env::var_os("PATH").as_deref()).ok_or(RuntimeDiagnostic::Unavailable)?;
    let mut version_command = Command::new(&executable);
    version_command.arg("--version").current_dir(&cwd);
    let mut version_process = OwnedProcess::spawn(&mut version_command).map_err(diagnostic)?;
    version_process.cancel_with(
        cancel.clone(),
        wake.try_clone()
            .map_err(|_| RuntimeDiagnostic::TransportFailed)?,
    );
    let raw = version(&mut version_process).map_err(diagnostic)?;
    let report = version_process.shutdown().map_err(diagnostic)?;
    let version = parse_version(&raw)
        .filter(|_| report.reaped && report.exit_code == Some(0))
        .ok_or(RuntimeDiagnostic::RuntimeRejected)?;
    if version != "0.159.0" {
        return Err(RuntimeDiagnostic::SafetyUnproven);
    }
    if cancel.load(Ordering::Acquire) {
        return Err(RuntimeDiagnostic::TransportFailed);
    }
    observe(instance, Observation::Discovering, cancel);
    // The namespace of an isolated generation is described once; discovery and the final
    // process are both launched inside it. Anything unsafe keeps the profile off.
    let local = match &generation.profile {
        LaunchProfile::Inference => None,
        LaunchProfile::LocalTools(isolation) => Some(
            (|| -> ProbeResult<Local<'_>> {
                Ok(Local {
                    isolation,
                    masked: isolation.layers()?,
                    roots: isolation.trust_roots()?,
                    home: text(isolation.home())?,
                    workspace: text(isolation.workspace.clone())?,
                })
            })()
            .map_err(|_| RuntimeDiagnostic::SafetyUnproven)?,
        ),
    };
    // Discovery has no thread-capable IO boundary and is reaped before the final generation.
    let mut names = discover_mcp(
        &executable,
        &cwd,
        cancel,
        wake.try_clone()
            .map_err(|_| RuntimeDiagnostic::TransportFailed)?,
        methods,
        local.as_ref(),
    )
    .map_err(|e| match e {
        ProbeError::Protocol if local.is_some() => RuntimeDiagnostic::SafetyUnproven,
        e => diagnostic(e),
    })?;
    names.extend(generation.forced_mcp.iter().cloned());
    // A private home configures no MCP server; one appearing there is not something to clamp.
    if local.is_some() && !names.is_empty() {
        return Err(RuntimeDiagnostic::SafetyUnproven);
    }
    // Discovery just refreshed the runtime's own backend catalog. The final generation only
    // ever sees the authority's normalized presentation of it.
    let catalog = authority_catalog(&generation.catalog_dir, local.as_ref())
        .map_err(|_| RuntimeDiagnostic::SafetyUnproven)?;
    let mut command = match &local {
        None => {
            let mut command = Command::new(&executable);
            command.args(
                safety::process_args(&names, Some(&catalog))
                    .map_err(|_| RuntimeDiagnostic::SafetyUnproven)?,
            );
            command
        }
        Some(local) => safety::local_tools_args(&names, Some(&catalog))
            .map_err(|_| ProbeError::Protocol)
            .and_then(|args| local.spawn(&executable, args))
            .map_err(|_| RuntimeDiagnostic::SafetyUnproven)?,
    };
    command.current_dir(&cwd);
    let mut owner = OwnedProcess::spawn(&mut command).map_err(diagnostic)?;
    owner.cancel_with(cancel.clone(), wake);
    observe(instance, Observation::Spawned, cancel);
    let mut report = initial_report(cwd);
    let mut attested = false;
    let result = (|| -> ProbeResult<()> {
        let mut forward = |event| observe(instance, event, cancel);
        let mut io = RuntimeIo {
            stdio: Stdio {
                owner: &mut owner,
                frames: Frames::default(),
                rejected: None,
                config_response_id: None,
            },
            methods,
            discovery: false,
            forward: Some(&mut forward),
            active_pair: None,
            isolation_violation: false,
            catalog: Some(&catalog),
            local: local.as_ref(),
            home_proven: false,
        };
        let mut correlation = handshake(&mut io, &mut report)?;
        // A catalog RPC error is safe degradation in the standalone probe, but not runtime readiness.
        // Supervision needs a validated catalog; fail with a fixed diagnostic, no inferred auth cause.
        if report.model_list_result != ProbeStage::Success {
            return Err(ProbeError::RuntimeRejected);
        }
        safety_preflight(
            &mut io,
            &names,
            report.cwd.to_str().ok_or(ProbeError::Protocol)?,
            &mut correlation,
            &mut initial_report(report.cwd.clone()),
        )?;
        let models = report.models.iter().map(|m| m.model.clone()).collect();
        let methods = io.methods.clone();
        io.emit(Observation::Ready {
            version,
            models,
            methods,
        });
        attested = true;
        io.stdio.owner.interruptible = true;
        // Infinite lifetime, finite per-frame deadline. Timeout merely permits an idle runtime;
        // cancellation wakes poll immediately. Counters saturate; maps and buffers remain bounded.
        loop {
            if let Ok(command) = commands.try_recv() {
                let uncertain = match command {
                    RuntimeCommand::Conversation(c) => {
                        let result = conversation(
                            &mut io,
                            &c,
                            &mut correlation,
                            &mut report,
                            cancel,
                            &names,
                        );
                        let unknown = result
                            .as_ref()
                            .is_err_and(|e| matches!(e, ConversationFailure::Unknown));
                        let methods = io.methods.clone();
                        io.emit(Observation::Conversation {
                            operation_id: c.operation_id,
                            result,
                            methods,
                        });
                        unknown
                    }
                    RuntimeCommand::Start(c) => {
                        let result = start_turn(&mut io, &c, &names, &mut correlation, &mut report);
                        let unknown = result
                            .as_ref()
                            .is_err_and(|e| matches!(e, ConversationFailure::Unknown));
                        let methods = io.methods.clone();
                        io.emit(Observation::TurnResult {
                            managed_turn_id: c.managed_turn_id,
                            result,
                            methods,
                        });
                        unknown
                    }
                    RuntimeCommand::Interrupt {
                        managed_turn_id,
                        thread_id,
                        turn_id,
                    } => {
                        let message = ClientOperation::TurnInterrupt { thread_id, turn_id }
                            .encode(CodexRequestId::String(uuid::Uuid::new_v4().to_string()))
                            .map_err(|_| ProbeError::Codec)?;
                        let success = exchange(
                            &mut io,
                            message,
                            Operation::TurnInterrupt,
                            Instant::now() + Duration::from_secs(3),
                            &mut correlation,
                            &mut report,
                        )
                        .is_ok();
                        let methods = io.methods.clone();
                        io.emit(Observation::InterruptResult {
                            managed_turn_id,
                            success,
                            methods,
                        });
                        !success
                    }
                };
                if uncertain {
                    return Err(ProbeError::Protocol);
                }
                // Another command may already be queued. Do not lose its wake to the RPC waiter.
                continue;
            }
            match io.receive(Instant::now() + Duration::from_secs(60)) {
                Ok(Some(message)) => {
                    correlation.notification_count = 0;
                    correlation.observe(&message, &mut report)?;
                }
                Err(ProbeError::Wake) => (),
                Ok(None) => return Err(ProbeError::UnexpectedEof),
                Err(ProbeError::Timeout) if io.stdio.frames.partial.is_empty() => (),
                Err(error) => return Err(error),
            }
        }
    })();
    // Reap before announcing exit; Drop is the last-resort guard on every earlier error path.
    let cleanup = owner.shutdown();
    if report.model_list_result == ProbeStage::RuntimeError {
        return Err(RuntimeDiagnostic::CatalogRejected);
    }
    result.and(cleanup.map(|_| ())).map_err(|e| match e {
        // An isolated generation that never proved its profile is unproven, whatever broke.
        ProbeError::Protocol if local.is_some() && !attested => RuntimeDiagnostic::SafetyUnproven,
        e => diagnostic(e),
    })
}

// Single outstanding provider request (bounded command queue=1). No completed-ID history grows.
// Any response outside this exact waiter fails the runtime, including duplicates/late responses.
fn conversation(
    io: &mut RuntimeIo<'_>,
    c: &ConversationCommand,
    correlation: &mut Correlation,
    report: &mut CodexLiveProbeReportV1,
    cancel: &AtomicBool,
    names: &BTreeSet<String>,
) -> std::result::Result<CodexThreadId, ConversationFailure> {
    let valid = c.admission.load(Ordering::Acquire)
        && crate::contracts::control::timestamp(&c.expires_at)
            .is_ok_and(|t| time::OffsetDateTime::now_utc() < t)
        && Path::new(&c.workspace).is_dir()
        && std::fs::canonicalize(&c.workspace).is_ok_and(|p| p == Path::new(&c.workspace))
        // An isolated generation serves exactly the workspace it was launched for.
        && io.local.is_none_or(|l| l.workspace == c.workspace);
    if !valid {
        return Err(ConversationFailure::NotDispatched);
    }
    safety_preflight(io, names, &c.workspace, correlation, report)
        .map_err(|_| ConversationFailure::Unknown)?;
    let mut message =
        managed_thread_request(c.request_id.clone(), &c.workspace, c.thread_id.as_ref())
            .map_err(|_| ConversationFailure::NotDispatched)?;
    // Inference fixes its posture when a thread is attached. A local-tools thread is never
    // created without it either: an unposed start would let the runtime trust the project.
    if c.thread_id.is_some() || io.local.is_some() {
        let mut v = message.vendor_value().clone();
        let (sandbox, policy, config) = match io.local {
            None => ("read-only", "never", safety::thread_config(names)),
            Some(local) => (
                "workspace-write",
                "untrusted",
                safety::local_tools_thread_config(names, &local.roots),
            ),
        };
        v["params"]["sandbox"] = serde_json::json!(sandbox);
        v["params"]["approvalPolicy"] = serde_json::json!(policy);
        v["params"]["config"] = config.map_err(|_| ConversationFailure::Unknown)?;
        message = WireMessage::from_value(v, Direction::ClientToServer)
            .map_err(|_| ConversationFailure::Unknown)?;
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    // External admission point. Subsequent durability steps finish this same admitted create;
    // they do not resume/attach or admit any new work after lease expiry/revocation.
    if !c.admission.load(Ordering::Acquire)
        || crate::contracts::control::timestamp(&c.expires_at)
            .map_or(true, |t| time::OffsetDateTime::now_utc() >= t)
    {
        return Err(ConversationFailure::NotDispatched);
    }
    let op = if c.thread_id.is_some() {
        Operation::ThreadResume
    } else {
        Operation::ThreadStart
    };
    let result = exchange(io, message, op, deadline, correlation, report)?;
    let thread = validate_conversation(&result, c)?;
    if io.local.is_some() {
        safety::verify_local_tools_thread(&result).map_err(|_| ConversationFailure::Unknown)?;
    }
    if c.thread_id.is_some() {
        if io.local.is_none() {
            safety::verify_resume(&result).map_err(|_| ConversationFailure::Unknown)?;
        }
        return Ok(thread);
    }
    checkpoint(
        io,
        c,
        &thread,
        MaterializationStage::Started,
        cancel,
        deadline,
    )?;
    for (suffix, unarchive, stage, op) in [
        (
            "archive",
            false,
            MaterializationStage::Archived,
            Operation::ThreadArchive,
        ),
        (
            "unarchive",
            true,
            MaterializationStage::Unarchived,
            Operation::ThreadUnarchive,
        ),
    ] {
        let CodexRequestId::String(base) = &c.request_id else {
            return Err(ConversationFailure::Unknown);
        };
        let id = CodexRequestId::String(format!("{base}-{suffix}"));
        let message = materialization_request(id, &thread, unarchive)
            .map_err(|_| ConversationFailure::Unknown)?;
        // Start already succeeded: every later failure is an uncertain provider effect.
        let result = exchange(io, message, op, deadline, correlation, report)
            .map_err(|_| ConversationFailure::Unknown)?;
        if unarchive
            && (result["thread"]["id"].as_str() != Some(thread.as_str())
                || result["thread"]["ephemeral"] != false
                || result["thread"]["cwd"].as_str().is_none_or(|cwd| {
                    !Path::new(cwd).is_absolute()
                        || std::fs::canonicalize(cwd).map_or(true, |p| p != Path::new(&c.workspace))
                }))
        {
            return Err(ConversationFailure::Unknown);
        }
        checkpoint(io, c, &thread, stage, cancel, deadline)?;
    }
    Ok(thread)
}
fn validate_conversation(
    r: &serde_json::Value,
    c: &ConversationCommand,
) -> std::result::Result<CodexThreadId, ConversationFailure> {
    if r["thread"]["ephemeral"] != false {
        return Err(ConversationFailure::Unknown);
    }
    let thread = CodexThreadId::parse(
        r["thread"]["id"]
            .as_str()
            .ok_or(ConversationFailure::Unknown)?,
    )
    .map_err(|_| ConversationFailure::Unknown)?;
    if c.thread_id.as_ref().is_some_and(|id| *id != thread) {
        return Err(ConversationFailure::Unknown);
    }
    for cwd in [r["cwd"].as_str(), r["thread"]["cwd"].as_str()] {
        let cwd = cwd.ok_or(ConversationFailure::Unknown)?;
        if !Path::new(cwd).is_absolute()
            || std::fs::canonicalize(cwd).map_or(true, |p| p != Path::new(&c.workspace))
        {
            return Err(ConversationFailure::Unknown);
        }
    }
    Ok(thread)
}
fn exchange(
    io: &mut RuntimeIo<'_>,
    message: WireMessage,
    op: Operation,
    deadline: Instant,
    correlation: &mut Correlation,
    report: &mut CodexLiveProbeReportV1,
) -> std::result::Result<serde_json::Value, ConversationFailure> {
    let id = message.id().ok_or(ConversationFailure::Unknown)?.clone();
    let mut bytes = message.encode().map_err(|_| ConversationFailure::Unknown)?;
    bytes.push(b'\n');
    io.send(&bytes, deadline)
        .map_err(|_| ConversationFailure::Unknown)?;
    loop {
        let m = match io.receive(deadline) {
            Ok(Some(m)) => m,
            Err(ProbeError::Wake) => continue,
            Err(_) => return Err(ConversationFailure::Unknown),
            Ok(None) => return Err(ConversationFailure::Unknown),
        };
        if matches!(
            m.kind(),
            WireKind::SuccessResponse | WireKind::ErrorResponse
        ) {
            validate_response(&m, &id, op).map_err(|_| ConversationFailure::Unknown)?;
            if m.kind() == WireKind::ErrorResponse {
                return Err(ConversationFailure::Rejected);
            }
            return Ok(m.result().unwrap().clone());
        }
        correlation.notification_count = 0;
        correlation
            .observe(&m, report)
            .map_err(|_| ConversationFailure::Unknown)?;
    }
}
fn checkpoint(
    io: &mut RuntimeIo<'_>,
    c: &ConversationCommand,
    thread: &CodexThreadId,
    stage: MaterializationStage,
    cancel: &AtomicBool,
    deadline: Instant,
) -> std::result::Result<(), ConversationFailure> {
    let (reply, rx) = mpsc::sync_channel(1);
    io.emit(Observation::Materialization {
        operation_id: c.operation_id.clone(),
        thread_id: thread.clone(),
        stage,
        reply,
    });
    // Bounded acknowledgement of the sole SQLite writer; no journal/database polling.
    loop {
        if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
            return Err(ConversationFailure::Unknown);
        }
        match rx.recv_timeout(
            Duration::from_millis(50).min(deadline.saturating_duration_since(Instant::now())),
        ) {
            Ok(true) => return Ok(()),
            Ok(false) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(ConversationFailure::Unknown);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => (),
        }
    }
}

fn safety_preflight(
    io: &mut RuntimeIo<'_>,
    names: &BTreeSet<String>,
    cwd: &str,
    correlation: &mut Correlation,
    report: &mut CodexLiveProbeReportV1,
) -> ProbeResult<()> {
    let config = safety_query(io, cwd, Query::Config, correlation, report)?;
    let catalog = io.catalog.ok_or(ProbeError::Protocol)?;
    match io.local {
        None => safety::verify_config(&config, catalog),
        Some(_) => safety::verify_local_tools_config(&config, catalog),
    }
    .map_err(|_| ProbeError::Protocol)?;
    let configured = safety::mcp_names(&config, false).map_err(|_| ProbeError::Protocol)?;
    if !configured.is_subset(names) {
        io.emit(Observation::SafetyConfigChanged(
            names.union(&configured).cloned().collect(),
        ));
        return Err(ProbeError::Protocol);
    }
    safety::mcp_names(&config, true).map_err(|_| ProbeError::Protocol)?;
    if let Some(local) = io.local {
        // The runtime reports its own managed requirements and its own home; the authority
        // re-reads the workspace: project policy that appeared or vanished since the
        // namespace was created is not masked by it.
        let requirements = safety_query(io, cwd, Query::Requirements, correlation, report)?;
        safety::verify_requirements(&requirements).map_err(|_| ProbeError::Protocol)?;
        if !io.home_proven || !local.isolation.masks_hold(&local.masked)? {
            return Err(ProbeError::Protocol);
        }
    }
    let hooks = safety_query(io, cwd, Query::Hooks, correlation, report)?;
    safety::zero_external_hooks(&hooks).map_err(|_| ProbeError::Protocol)
}
#[derive(Clone, Copy, PartialEq)]
enum Query {
    Config,
    Requirements,
    Hooks,
}
fn safety_query(
    io: &mut RuntimeIo<'_>,
    cwd: &str,
    query: Query,
    correlation: &mut Correlation,
    report: &mut CodexLiveProbeReportV1,
) -> ProbeResult<serde_json::Value> {
    let id = CodexRequestId::String(uuid::Uuid::new_v4().to_string());
    let (method, params, op) = match query {
        Query::Hooks => (
            "hooks/list",
            serde_json::json!({"cwds":[cwd]}),
            Operation::HooksList,
        ),
        Query::Config => (
            "config/read",
            serde_json::json!({"cwd":cwd,"includeLayers":false}),
            Operation::ConfigRead,
        ),
        Query::Requirements => (
            "configRequirements/read",
            serde_json::json!({}),
            Operation::ConfigRequirementsRead,
        ),
    };
    if query != Query::Hooks {
        io.stdio.config_response_id = Some(id.clone());
    }
    let message = WireMessage::request(id, method, params).map_err(|_| ProbeError::Codec)?;
    let result = exchange(
        io,
        message,
        op,
        Instant::now() + REQUEST_TIMEOUT,
        correlation,
        report,
    )
    .map_err(|_| {
        if io.isolation_violation {
            ProbeError::InferenceIsolationViolation
        } else {
            ProbeError::Protocol
        }
    });
    io.stdio.config_response_id = None;
    result
}
fn text(path: std::path::PathBuf) -> ProbeResult<String> {
    path.into_os_string()
        .into_string()
        .map_err(|_| ProbeError::Protocol)
}
fn discover_mcp(
    executable: &Path,
    cwd: &Path,
    cancel: &Arc<AtomicBool>,
    wake: UnixStream,
    methods: &mut RuntimeMethods,
    local: Option<&Local<'_>>,
) -> ProbeResult<BTreeSet<String>> {
    let mut command = match local {
        None => {
            let mut command = Command::new(executable);
            command
                .args(safety::process_args(&BTreeSet::new(), None).map_err(|_| ProbeError::Codec)?);
            command
        }
        Some(local) => local.spawn(
            executable,
            safety::local_tools_args(&BTreeSet::new(), None).map_err(|_| ProbeError::Codec)?,
        )?,
    };
    command.current_dir(cwd);
    let mut owner = OwnedProcess::spawn(&mut command)?;
    owner.cancel_with(cancel.clone(), wake);
    let result = (|| {
        let mut io = RuntimeIo {
            stdio: Stdio {
                owner: &mut owner,
                frames: Frames::default(),
                rejected: None,
                config_response_id: None,
            },
            methods,
            discovery: true,
            forward: None,
            active_pair: None,
            isolation_violation: false,
            catalog: None,
            local,
            home_proven: false,
        };
        let mut report = initial_report(cwd.to_owned());
        let mut correlation = handshake(&mut io, &mut report)?;
        if report.model_list_result != ProbeStage::Success {
            return Err(ProbeError::RuntimeRejected);
        }
        let config = safety_query(
            &mut io,
            cwd.to_str().ok_or(ProbeError::Protocol)?,
            Query::Config,
            &mut correlation,
            &mut report,
        )?;
        if local.is_some() && !io.home_proven {
            return Err(ProbeError::Protocol);
        }
        safety::mcp_names(&config, false).map_err(|_| ProbeError::Protocol)
    })();
    owner.shutdown()?;
    result
}

/// Reads the runtime's own cached backend catalog and writes the authority presentation
/// next to the control socket. Wrong owner, symlink, oversize or malformed content keeps
/// the runtime from starting; nothing is inferred from a missing cache.
fn authority_catalog(dir: &Path, local: Option<&Local<'_>>) -> ProbeResult<std::path::PathBuf> {
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let flags = nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC;
    // Each profile reads the cache its own discovery just refreshed, in its own home.
    let (home, name) = match local {
        None => (isolation::user_home()?, "codex-model-catalog.json"),
        Some(local) => (local.isolation.home(), "codex-local-tools-catalog.json"),
    };
    let cache = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags)
        .open(home.join("models_cache.json"))
        .map_err(|_| ProbeError::Protocol)?;
    let metadata = cache.metadata().map_err(|_| ProbeError::Protocol)?;
    if !metadata.is_file()
        || metadata.uid() != nix::unistd::geteuid().as_raw()
        || metadata.len() > safety::MAX_CATALOG_BYTES as u64
    {
        return Err(ProbeError::Protocol);
    }
    let mut raw = Vec::new();
    cache
        .take(safety::MAX_CATALOG_BYTES as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|_| ProbeError::Protocol)?;
    let presented = match local {
        None => safety::inference_catalog(&raw),
        Some(_) => safety::local_tools_catalog(&raw),
    }
    .map_err(|_| ProbeError::Protocol)?;
    let path = dir.join(name);
    if !safety::safe_catalog_path(&path) {
        return Err(ProbeError::Protocol);
    }
    let staging = dir.join(format!("{name}.tmp"));
    let _ = std::fs::remove_file(&staging);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(flags)
        .open(&staging)
        .map_err(|_| ProbeError::Protocol)?;
    file.write_all(&presented)
        .and_then(|()| file.sync_all())
        .and_then(|()| std::fs::rename(&staging, &path))
        .map_err(|_| ProbeError::Protocol)?;
    Ok(path)
}

fn start_turn(
    io: &mut RuntimeIo<'_>,
    c: &TurnCommand,
    names: &BTreeSet<String>,
    correlation: &mut Correlation,
    report: &mut CodexLiveProbeReportV1,
) -> std::result::Result<CodexTurnId, ConversationFailure> {
    if io.local.is_some_and(|l| l.workspace != c.workspace) {
        return Err(ConversationFailure::NotDispatched);
    }
    safety_preflight(io, names, &c.workspace, correlation, report)
        .map_err(|_| ConversationFailure::Unknown)?;
    if !c.admission.load(Ordering::Acquire)
        || crate::contracts::control::timestamp(&c.expires_at)
            .map_or(true, |t| time::OffsetDateTime::now_utc() >= t)
    {
        return Err(ConversationFailure::NotDispatched);
    }
    let message = ClientOperation::TurnStart {
        thread_id: c.thread_id.clone(),
        text: c.prompt.clone(),
    }
    .encode(CodexRequestId::String(uuid::Uuid::new_v4().to_string()))
    .map_err(|_| ConversationFailure::NotDispatched)?;
    let result = exchange(
        io,
        message,
        Operation::TurnStart,
        Instant::now() + REQUEST_TIMEOUT,
        correlation,
        report,
    )?;
    if result["turn"]["status"] != "inProgress" {
        return Err(ConversationFailure::Unknown);
    }
    let turn = CodexTurnId::parse(
        result["turn"]["id"]
            .as_str()
            .ok_or(ConversationFailure::Unknown)?,
    )
    .map_err(|_| ConversationFailure::Unknown)?;
    io.active_pair = Some((c.thread_id.clone(), turn.clone()));
    Ok(turn)
}
