//! Shared 2C transport/handshake, retained under the authority after catalog readiness.
use super::*;
use crate::contracts::{
    UtcTimestamp, WireUuid,
    control::{RuntimeDiagnostic, RuntimeMethods},
    identities::AgentRuntimeInstanceId,
};
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
pub(crate) struct Worker {
    cancel: Arc<AtomicBool>,
    wake: UnixStream,
    thread: Option<JoinHandle<RuntimeMethods>>,
    commands: SyncSender<ConversationCommand>,
}
impl Worker {
    /// Fixed discovery, argv and cwd. No client-provided process inputs.
    pub fn start(
        instance: AgentRuntimeInstanceId,
        mut observe: impl FnMut(&AgentRuntimeInstanceId, Observation, &AtomicBool) + Send + 'static,
    ) -> std::io::Result<Self> {
        let (wake, reader) = UnixStream::pair()?;
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let (commands, rx) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("codex-runtime".into())
            .spawn(move || {
                let mut methods = RuntimeMethods::default();
                let result = run(&instance, &flag, reader, rx, &mut observe, &mut methods);
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
    pub fn command(&mut self, command: ConversationCommand) -> bool {
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
}
impl ProbeIo for RuntimeIo<'_> {
    fn send(&mut self, bytes: &[u8], deadline: Instant) -> ProbeResult<()> {
        let message = WireMessage::parse(
            bytes.strip_suffix(b"\n").ok_or(ProbeError::Codec)?,
            Direction::ClientToServer,
        )
        .map_err(|_| ProbeError::Codec)?;
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
            let valid = if message.method() == Some("thread/start") {
                p.len() == 2 && p.get("ephemeral") == Some(&serde_json::json!(false))
            } else {
                p.len() == 3
                    && p.get("excludeTurns") == Some(&serde_json::json!(true))
                    && p.get("threadId")
                        .and_then(|v| v.as_str())
                        .is_some_and(|id| CodexThreadId::parse(id).is_ok())
            };
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
        if *counter != 0 {
            return Err(ProbeError::Protocol);
        }
        self.stdio.send(bytes, deadline)?;
        *counter += 1;
        Ok(())
    }
    fn receive(&mut self, deadline: Instant) -> ProbeResult<Option<WireMessage>> {
        self.stdio.receive(deadline)
    }
    fn close_input(&mut self) {
        self.stdio.close_input();
    }
}
fn run(
    instance: &AgentRuntimeInstanceId,
    cancel: &Arc<AtomicBool>,
    wake: UnixStream,
    commands: Receiver<ConversationCommand>,
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
    if cancel.load(Ordering::Acquire) {
        return Err(RuntimeDiagnostic::TransportFailed);
    }
    let mut command = Command::new(&executable);
    command.arg("app-server").current_dir(&cwd);
    let mut owner = OwnedProcess::spawn(&mut command).map_err(diagnostic)?;
    owner.cancel_with(cancel.clone(), wake);
    observe(instance, Observation::Spawned, cancel);
    let mut report = initial_report(cwd);
    let result = (|| -> ProbeResult<()> {
        let mut io = RuntimeIo {
            stdio: Stdio {
                owner: &mut owner,
                frames: Frames::default(),
                rejected: None,
            },
            methods,
        };
        let mut correlation = handshake(&mut io, &mut report)?;
        // A catalog RPC error is safe degradation in the standalone probe, but not runtime readiness.
        // Supervision needs a validated catalog; fail with a fixed diagnostic, no inferred auth cause.
        if report.model_list_result != ProbeStage::Success {
            return Err(ProbeError::RuntimeRejected);
        }
        let models = report.models.iter().map(|m| m.model.clone()).collect();
        let methods = io.methods.clone();
        observe(
            instance,
            Observation::Ready {
                version,
                models,
                methods,
            },
            cancel,
        );
        io.stdio.owner.interruptible = true;
        // Infinite lifetime, finite per-frame deadline. Timeout merely permits an idle runtime;
        // cancellation wakes poll immediately. Counters saturate; maps and buffers remain bounded.
        loop {
            if let Ok(command) = commands.try_recv() {
                let result = conversation(
                    &mut io,
                    &command,
                    &mut correlation,
                    &mut report,
                    instance,
                    observe,
                    cancel,
                );
                let uncertain = result
                    .as_ref()
                    .is_err_and(|e| matches!(e, ConversationFailure::Unknown));
                observe(
                    instance,
                    Observation::Conversation {
                        operation_id: command.operation_id,
                        result,
                        methods: io.methods.clone(),
                    },
                    cancel,
                );
                if uncertain {
                    return Err(ProbeError::Protocol);
                }
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
    result.and(cleanup.map(|_| ())).map_err(diagnostic)
}

// Single outstanding provider request (bounded command queue=1). No completed-ID history grows.
// Any response outside this exact waiter fails the runtime, including duplicates/late responses.
fn conversation(
    io: &mut RuntimeIo<'_>,
    c: &ConversationCommand,
    correlation: &mut Correlation,
    report: &mut CodexLiveProbeReportV1,
    instance: &AgentRuntimeInstanceId,
    observe: &mut impl FnMut(&AgentRuntimeInstanceId, Observation, &AtomicBool),
    cancel: &AtomicBool,
) -> std::result::Result<CodexThreadId, ConversationFailure> {
    let valid = c.admission.load(Ordering::Acquire)
        && crate::contracts::control::timestamp(&c.expires_at)
            .is_ok_and(|t| time::OffsetDateTime::now_utc() < t)
        && Path::new(&c.workspace).is_dir()
        && std::fs::canonicalize(&c.workspace).is_ok_and(|p| p == Path::new(&c.workspace));
    if !valid {
        return Err(ConversationFailure::NotDispatched);
    }
    let message = managed_thread_request(c.request_id.clone(), &c.workspace, c.thread_id.as_ref())
        .map_err(|_| ConversationFailure::NotDispatched)?;
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
    if c.thread_id.is_some() {
        return Ok(thread);
    }
    checkpoint(
        instance,
        c,
        &thread,
        MaterializationStage::Started,
        observe,
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
        checkpoint(instance, c, &thread, stage, observe, cancel, deadline)?;
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
            _ => return Err(ConversationFailure::Unknown),
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
    instance: &AgentRuntimeInstanceId,
    c: &ConversationCommand,
    thread: &CodexThreadId,
    stage: MaterializationStage,
    observe: &mut impl FnMut(&AgentRuntimeInstanceId, Observation, &AtomicBool),
    cancel: &AtomicBool,
    deadline: Instant,
) -> std::result::Result<(), ConversationFailure> {
    let (reply, rx) = mpsc::sync_channel(1);
    observe(
        instance,
        Observation::Materialization {
            operation_id: c.operation_id.clone(),
            thread_id: thread.clone(),
            stage,
            reply,
        },
        cancel,
    );
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
