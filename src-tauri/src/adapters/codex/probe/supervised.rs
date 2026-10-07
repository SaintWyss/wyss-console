//! Shared 2C transport/handshake, retained under the authority after catalog readiness.
use super::*;
use crate::contracts::{
    control::{RuntimeDiagnostic, RuntimeMethods},
    identities::AgentRuntimeInstanceId,
};
use std::{
    io::Write,
    os::unix::net::UnixStream,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

pub(crate) enum Observation {
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
        let thread = thread::Builder::new()
            .name("codex-runtime".into())
            .spawn(move || {
                let mut methods = RuntimeMethods::default();
                let result = run(&instance, &flag, reader, &mut observe, &mut methods);
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
        })
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
        // Infinite lifetime, finite per-frame deadline. Timeout merely permits an idle runtime;
        // cancellation wakes poll immediately. Counters saturate; maps and buffers remain bounded.
        loop {
            match io.receive(Instant::now() + Duration::from_secs(60)) {
                Ok(Some(message)) => {
                    correlation.notification_count = 0;
                    correlation.observe(&message, &mut report)?;
                }
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
