//! Explicit developer-only read-only compatibility probe. No Tauri entrypoint.
//! Production executable/argv/cwd and outbound plan are fixed here, not caller inputs.
#[cfg(target_os = "linux")]
pub(crate) mod isolation;
mod process;
#[cfg(target_os = "linux")]
pub(crate) mod supervised;
#[cfg(test)]
mod tests;

use super::*;
use crate::contracts::agent_adapter::{AgentModelDescriptorV1, CompatibilityState};
use process::{OwnedProcess, ShutdownReport};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const VERSION_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const VERSION_LIMIT: usize = 4096;
const MODEL_LIMIT: u32 = 50;
const NOTIFICATION_LIMIT: u32 = 1024;
const METHOD_LIMIT: usize = 64;
const PINNED_SCHEMA: &str = "0b863c69f50335acd92164aab971cb58d298c2fe";
type ProbeResult<T> = std::result::Result<T, ProbeError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ProbeError {
    RuntimeUnavailable,
    RuntimeRejected,
    Spawn,
    Transport,
    Timeout,
    Protocol,
    WireRejected(&'static str),
    OversizedFrame,
    UnexpectedEof,
    UnexpectedServerRequest,
    UnrelatedResponse,
    DuplicateResponse,
    NotificationLimit,
    Shutdown,
    VersionOutput,
    Codec,
    Wake,
    InferenceIsolationViolation,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeStage {
    NotAttempted,
    Success,
    RuntimeError,
    Failed,
}
#[derive(Debug, Serialize)]
pub struct ProbeModel {
    model: AgentModelDescriptorV1,
    hidden: bool,
    is_default: bool,
    default_reasoning_effort: String,
    reasoning_efforts: Vec<String>,
}
/// Safe allowlisted output only. No raw provider error, stderr, response or auth data.
/// This is an internal developer report; it is not a frontend or durable DTO.
#[derive(Debug, Serialize)]
pub struct CodexLiveProbeReportV1 {
    schema_version: u8,
    timestamp_unix_ms: u64,
    pinned_schema_revision: &'static str,
    executable_found: bool,
    executable: Option<PathBuf>,
    cwd: PathBuf,
    runtime_version: Option<String>,
    parsed_runtime_version: Option<String>,
    version_result: ProbeStage,
    version_shutdown: Option<ShutdownReport>,
    app_server_pid: Option<u32>,
    spawn_result: ProbeStage,
    initialization_result: ProbeStage,
    initialized_sent: bool,
    model_list_result: ProbeStage,
    model_list_parameters: serde_json::Value,
    models: Vec<ProbeModel>,
    next_cursor_present: bool,
    compatibility: CompatibilityState,
    initialize_request_id: Option<CodexRequestId>,
    model_request_id: Option<CodexRequestId>,
    confirmed_evidence: Vec<&'static str>,
    notifications: BTreeMap<String, u32>,
    notification_shapes: BTreeMap<String, WireStructure>,
    rejected_frame: Option<WireStructure>,
    unexpected_server_request: bool,
    vendor_error_code: Option<i64>,
    codec_error: Option<&'static str>,
    diagnostics: Vec<ProbeError>,
    stderr_bytes_consumed: u64,
    shutdown: Option<ShutdownReport>,
}
impl CodexLiveProbeReportV1 {
    pub fn runtime_found(&self) -> bool {
        self.executable_found
    }
    pub fn successful_probe(&self) -> bool {
        matches!(
            self.compatibility,
            CompatibilityState::Compatible | CompatibilityState::Degraded
        ) && self.diagnostics.is_empty()
            && matches!(
                self.model_list_result,
                ProbeStage::Success | ProbeStage::RuntimeError
            )
            && self.shutdown.as_ref().is_some_and(|s| s.reaped)
            && !self.unexpected_server_request
    }
}

fn discover(path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    env::split_paths(path?)
        .filter(|p| p.is_absolute())
        .find_map(|directory| {
            let candidate = directory.join("codex");
            let metadata = fs::metadata(&candidate).ok()?;
            if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
                return None;
            }
            candidate.canonicalize().ok()
        })
}

/// Incremental JSONL reader: one partial frame plus one 4 KiB scratch buffer.
/// A newline is framing, not part of the 256 KiB message allowance.
struct Frames {
    partial: Vec<u8>,
    scratch: [u8; 4096],
    position: usize,
    length: usize,
}
impl Default for Frames {
    fn default() -> Self {
        Self {
            partial: Vec::new(),
            scratch: [0; 4096],
            position: 0,
            length: 0,
        }
    }
}
trait ProbeIo {
    fn send(&mut self, bytes: &[u8], deadline: Instant) -> ProbeResult<()>;
    fn receive(&mut self, deadline: Instant) -> ProbeResult<Option<WireMessage>>;
    fn close_input(&mut self);
}
struct Stdio<'a> {
    owner: &'a mut OwnedProcess,
    frames: Frames,
    rejected: Option<WireStructure>,
    config_response_id: Option<CodexRequestId>,
}
impl ProbeIo for Stdio<'_> {
    fn send(&mut self, bytes: &[u8], deadline: Instant) -> ProbeResult<()> {
        self.owner.write(bytes, deadline)
    }
    fn close_input(&mut self) {
        self.owner.close_input();
    }
    fn receive(&mut self, deadline: Instant) -> ProbeResult<Option<WireMessage>> {
        loop {
            if Instant::now() >= deadline {
                return Err(ProbeError::Timeout);
            }
            while self.frames.position < self.frames.length {
                let byte = self.frames.scratch[self.frames.position];
                self.frames.position += 1;
                if byte == b'\n' {
                    let message = WireMessage::parse_diagnosed_for_config(
                        &self.frames.partial,
                        Direction::ServerToClient,
                        self.config_response_id.as_ref(),
                    )
                    .map_err(|e| {
                        self.rejected = e.structure;
                        ProbeError::WireRejected(e.error.code)
                    });
                    self.frames.partial.clear();
                    return message.map(Some);
                }
                if self.frames.partial.len() == MAX_FRAME_BYTES {
                    return Err(ProbeError::OversizedFrame);
                }
                self.frames.partial.push(byte);
            }
            match self.owner.read(&mut self.frames.scratch)? {
                Some(0) if self.frames.partial.is_empty() => return Ok(None),
                Some(0) => return Err(ProbeError::UnexpectedEof),
                Some(n) => {
                    self.frames.position = 0;
                    self.frames.length = n;
                }
                None => self.owner.wait(deadline, false)?,
            }
        }
    }
}

// Only these three outbound messages can be constructed by the live probe.
// This is intentionally not a general ClientOperation/approval transport API.
struct ProbePlan {
    initialize: WireMessage,
    initialized: WireMessage,
    models: WireMessage,
}
impl ProbePlan {
    fn new() -> ProbeResult<Self> {
        let nonce = Uuid::new_v4();
        let id = |suffix| CodexRequestId::String(format!("wyss-probe-{nonce}-{suffix}"));
        Ok(Self {
            initialize: ClientOperation::Initialize {
                client_info: ClientInfo {
                    name: "wyss_console".into(),
                    title: Some("Wyss Console".into()),
                    version: env!("CARGO_PKG_VERSION").into(),
                },
                experimental_api: false,
            }
            .encode(id("initialize"))
            .map_err(|_| ProbeError::Codec)?,
            initialized: initialized_notification().map_err(|_| ProbeError::Codec)?,
            models: ClientOperation::ModelList {
                limit: Some(MODEL_LIMIT),
                cursor: None,
                include_hidden: false,
            }
            .encode(id("models"))
            .map_err(|_| ProbeError::Codec)?,
        })
    }
}
fn send(
    io: &mut impl ProbeIo,
    state: &mut ProtocolState,
    message: &WireMessage,
) -> ProbeResult<()> {
    state.outbound(message).map_err(|_| ProbeError::Protocol)?;
    let mut bytes = message.encode().map_err(|_| ProbeError::Codec)?;
    bytes.push(b'\n');
    io.send(&bytes, Instant::now() + REQUEST_TIMEOUT)
}
struct Correlation {
    completed: Vec<CodexRequestId>,
    notification_count: u32,
}
impl Correlation {
    fn observe(
        &mut self,
        message: &WireMessage,
        report: &mut CodexLiveProbeReportV1,
    ) -> ProbeResult<()> {
        match message.kind() {
            WireKind::ServerRequest => {
                report.unexpected_server_request = true;
                Err(ProbeError::UnexpectedServerRequest)
            }
            WireKind::Notification => {
                self.notification_count += 1;
                if self.notification_count > NOTIFICATION_LIMIT {
                    return Err(ProbeError::NotificationLimit);
                }
                normalize_inbound(message, None).map_err(|_| ProbeError::Protocol)?;
                let method = message.method().unwrap();
                if !report.notifications.contains_key(method)
                    && report.notifications.len() == METHOD_LIMIT
                {
                    return Err(ProbeError::NotificationLimit);
                }
                report
                    .notification_shapes
                    .entry(method.to_owned())
                    .or_insert_with(|| message.structure());
                let count = report.notifications.entry(method.to_owned()).or_default();
                *count = count.saturating_add(1);
                Ok(())
            }
            _ => {
                if message.id().is_some_and(|id| self.completed.contains(id)) {
                    Err(ProbeError::DuplicateResponse)
                } else {
                    Err(ProbeError::UnrelatedResponse)
                }
            }
        }
    }
    fn response(
        &mut self,
        io: &mut impl ProbeIo,
        id: &CodexRequestId,
        operation: Operation,
        report: &mut CodexLiveProbeReportV1,
    ) -> ProbeResult<WireMessage> {
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        loop {
            let message = io.receive(deadline)?.ok_or(ProbeError::UnexpectedEof)?;
            if matches!(
                message.kind(),
                WireKind::SuccessResponse | WireKind::ErrorResponse
            ) && message.id() == Some(id)
            {
                if self.completed.contains(id) {
                    return Err(ProbeError::DuplicateResponse);
                }
                validate_response(&message, id, operation).map_err(|e| {
                    report.codec_error = Some(e.code);
                    ProbeError::Protocol
                })?;
                // Fixed two-request plan bounds this list to two IDs.
                self.completed.push(id.clone());
                return Ok(message);
            }
            self.observe(&message, report)?;
        }
    }
}
fn wire_error_code(message: &WireMessage) -> Option<i64> {
    message.vendor_value().get("error")?.get("code")?.as_i64()
}
fn handshake(
    io: &mut impl ProbeIo,
    report: &mut CodexLiveProbeReportV1,
) -> ProbeResult<Correlation> {
    let plan = ProbePlan::new()?;
    report.initialize_request_id = plan.initialize.id().cloned();
    report.model_request_id = plan.models.id().cloned();
    let mut state = ProtocolState::default();
    let mut correlation = Correlation {
        completed: Vec::with_capacity(2),
        notification_count: 0,
    };
    send(io, &mut state, &plan.initialize)?;
    let response = correlation.response(
        io,
        plan.initialize.id().unwrap(),
        Operation::Initialize,
        report,
    )?;
    state
        .initialize_response(&response)
        .map_err(|_| ProbeError::Protocol)?;
    if response.kind() == WireKind::ErrorResponse {
        report.initialization_result = ProbeStage::RuntimeError;
        report.vendor_error_code = wire_error_code(&response);
        return Err(ProbeError::RuntimeRejected);
    }
    send(io, &mut state, &plan.initialized)?;
    report.initialization_result = ProbeStage::Success;
    report.initialized_sent = true;
    report
        .confirmed_evidence
        .extend(["structured_stdio", "initialize_handshake"]);
    report.compatibility = CompatibilityState::Compatible;
    send(io, &mut state, &plan.models)?;
    let response =
        correlation.response(io, plan.models.id().unwrap(), Operation::ModelList, report)?;
    if response.kind() == WireKind::ErrorResponse {
        report.model_list_result = ProbeStage::RuntimeError;
        report.vendor_error_code = wire_error_code(&response);
        // Generic RPC codes don't establish authentication/configuration causes.
        // Do not copy raw messages or infer causes from English text.
        report.compatibility = CompatibilityState::Degraded;
    } else {
        let CodexObservation::ModelCatalog {
            models,
            next_cursor,
        } = normalize_response(
            &response,
            plan.models.id().unwrap(),
            Operation::ModelList,
            None,
        )
        .map_err(|_| ProbeError::Protocol)?
        else {
            return Err(ProbeError::Protocol);
        };
        if models.len() > MODEL_LIMIT as usize {
            return Err(ProbeError::Protocol);
        }
        report.models = models
            .into_iter()
            .map(|m| ProbeModel {
                model: m.model,
                hidden: m.codex["hidden"].as_bool().unwrap(),
                is_default: m.codex["isDefault"].as_bool().unwrap(),
                default_reasoning_effort: m.codex["defaultReasoningEffort"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
                reasoning_efforts: m.codex["supportedReasoningEfforts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| e["reasoningEffort"].as_str().unwrap().to_owned())
                    .collect(),
            })
            .collect();
        report.next_cursor_present = next_cursor.is_some();
        report.model_list_result = ProbeStage::Success;
        report.confirmed_evidence.push("model_list.observe");
    }
    Ok(correlation)
}
fn exchange(io: &mut impl ProbeIo, report: &mut CodexLiveProbeReportV1) -> ProbeResult<()> {
    let mut correlation = handshake(io, report)?;
    // EOF requests graceful exit. Validate final/interleaved frames, including duplicate
    // completions, until EOF or a bounded grace deadline. No more outbound operations.
    io.close_input();
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match io.receive(deadline) {
            Ok(Some(message)) => correlation.observe(&message, report)?,
            Ok(None) | Err(ProbeError::Timeout) => break,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
fn initial_report(cwd: PathBuf) -> CodexLiveProbeReportV1 {
    CodexLiveProbeReportV1 {
        schema_version: 1,
        timestamp_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u64::MAX as u128) as u64,
        pinned_schema_revision: PINNED_SCHEMA,
        executable_found: false,
        executable: None,
        cwd,
        runtime_version: None,
        parsed_runtime_version: None,
        version_result: ProbeStage::NotAttempted,
        version_shutdown: None,
        app_server_pid: None,
        spawn_result: ProbeStage::NotAttempted,
        initialization_result: ProbeStage::NotAttempted,
        initialized_sent: false,
        model_list_result: ProbeStage::NotAttempted,
        model_list_parameters: serde_json::json!({"limit":MODEL_LIMIT,"cursor":null,"includeHidden":false}),
        models: Vec::new(),
        next_cursor_present: false,
        compatibility: CompatibilityState::Unknown,
        initialize_request_id: None,
        model_request_id: None,
        confirmed_evidence: Vec::new(),
        notifications: BTreeMap::new(),
        notification_shapes: BTreeMap::new(),
        rejected_frame: None,
        unexpected_server_request: false,
        vendor_error_code: None,
        codec_error: None,
        diagnostics: Vec::new(),
        stderr_bytes_consumed: 0,
        shutdown: None,
    }
}
fn apply_failure(report: &mut CodexLiveProbeReportV1, error: ProbeError) {
    report.diagnostics.push(error);
    report.compatibility = match error {
        ProbeError::Protocol
        | ProbeError::WireRejected(_)
        | ProbeError::OversizedFrame
        | ProbeError::Codec
        | ProbeError::DuplicateResponse
        | ProbeError::UnrelatedResponse => CompatibilityState::Incompatible,
        _ if report.initialized_sent => CompatibilityState::Degraded,
        _ => CompatibilityState::Unknown,
    };
    if report.initialization_result == ProbeStage::NotAttempted {
        report.initialization_result = ProbeStage::Failed;
    } else if report.model_list_result == ProbeStage::NotAttempted {
        report.model_list_result = ProbeStage::Failed;
    }
}
fn version(owner: &mut OwnedProcess) -> ProbeResult<String> {
    owner.close_input();
    version_until(owner, Instant::now() + VERSION_TIMEOUT)
}
fn version_until(owner: &mut OwnedProcess, deadline: Instant) -> ProbeResult<String> {
    let mut bytes = Vec::new();
    let mut scratch = [0; 4096];
    loop {
        if Instant::now() >= deadline {
            return Err(ProbeError::Timeout);
        }
        match owner.read(&mut scratch)? {
            Some(0) => break,
            Some(n) => {
                if bytes.len() + n > VERSION_LIMIT {
                    return Err(ProbeError::VersionOutput);
                }
                bytes.extend_from_slice(&scratch[..n]);
            }
            None => owner.wait(deadline, false)?,
        }
    }
    let value = String::from_utf8(bytes).map_err(|_| ProbeError::VersionOutput)?;
    let value = value.trim();
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(ProbeError::VersionOutput);
    }
    Ok(value.into())
}

fn parse_version(raw: &str) -> Option<String> {
    let value = raw.strip_prefix("codex-cli ")?;
    let parts: Vec<_> = value.split('.').collect();
    (parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())))
    .then(|| value.to_owned())
}

/// Fixed local discovery -> version -> read-only handshake/catalog -> owned cleanup.
/// No executable, arguments, environment overrides, PIDs or work requests are accepted.
pub fn run_codex_live_probe() -> CodexLiveProbeReportV1 {
    let cwd = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let mut report = initial_report(cwd);
    let Some(executable) = discover(env::var_os("PATH").as_deref()) else {
        report.diagnostics.push(ProbeError::RuntimeUnavailable);
        return report;
    };
    report.executable_found = true;
    report.executable = Some(executable.clone());
    let mut command = Command::new(&executable);
    command.arg("--version").current_dir(&report.cwd);
    let mut owner = match OwnedProcess::spawn(&mut command) {
        Ok(owner) => owner,
        Err(e) => {
            report.version_result = ProbeStage::Failed;
            report.diagnostics.push(e);
            return report;
        }
    };
    let result = version(&mut owner);
    let shutdown = owner.shutdown();
    report.stderr_bytes_consumed = owner.stderr_bytes;
    match (result, shutdown) {
        (Ok(value), Ok(shutdown))
            if shutdown.exit_code == Some(0) && !shutdown.terminated && !shutdown.killed =>
        {
            report.parsed_runtime_version = parse_version(&value);
            report.runtime_version = Some(value);
            report.version_result = ProbeStage::Success;
            report.version_shutdown = Some(shutdown);
        }
        (result, shutdown) => {
            report.version_result = ProbeStage::Failed;
            report
                .diagnostics
                .push(result.err().unwrap_or(ProbeError::VersionOutput));
            match shutdown {
                Ok(s) => report.version_shutdown = Some(s),
                Err(e) => report.diagnostics.push(e),
            }
            return report;
        }
    }
    drop(owner);
    let mut command = Command::new(executable);
    command.arg("app-server").current_dir(&report.cwd);
    let mut owner = match OwnedProcess::spawn(&mut command) {
        Ok(owner) => owner,
        Err(e) => {
            report.spawn_result = ProbeStage::Failed;
            report.diagnostics.push(e);
            return report;
        }
    };
    report.app_server_pid = Some(owner.pid());
    report.spawn_result = ProbeStage::Success;
    let result = {
        let mut io = Stdio {
            owner: &mut owner,
            frames: Frames::default(),
            rejected: None,
            config_response_id: None,
        };
        let result = exchange(&mut io, &mut report);
        report.rejected_frame = io.rejected.take();
        result
    };
    if let Err(e) = result {
        apply_failure(&mut report, e);
    }
    match owner.shutdown() {
        Ok(shutdown) => report.shutdown = Some(shutdown),
        Err(e) => {
            report.diagnostics.push(e);
            report.compatibility = CompatibilityState::Degraded;
        }
    }
    report.stderr_bytes_consumed = report
        .stderr_bytes_consumed
        .saturating_add(owner.stderr_bytes);
    report
}
