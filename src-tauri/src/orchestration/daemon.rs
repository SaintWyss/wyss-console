//! Explicitly started, per-user headless coordinator process. It is not installed as a service.
use super::{
    Coordinator, CoordinatorError, HandoffConfirmationV1, HandoffEnvelopeV1, OrchestrationPlanV1,
    PlanReviewV1, RunJournal,
    scheduler::{F8Authority, ReconcileDecision},
    workspace_identity, workspace_revision,
};
use crate::control::paths::{Paths, private_dir};
use nix::{
    fcntl::{Flock, FlockArg},
    unistd::geteuid,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    env,
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        net::{UnixListener, UnixStream},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const SOCKET_NAME: &str = "orchestration.sock";
const DATABASE_NAME: &str = "orchestration.sqlite3";
const LOCK_NAME: &str = "orchestration.lock";
const MAX_FRAME: usize = 2 * 1024 * 1024;
const MAX_REQUEST: usize = 256 * 1024;
const TICK: Duration = Duration::from_millis(250);
const IDLE_EXIT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "method",
    content = "params",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum DaemonRequest {
    WorkspaceSnapshot {
        workspace_root: String,
    },
    ReviewPlan {
        plan: OrchestrationPlanV1,
    },
    SealHandoff {
        envelope: HandoffEnvelopeV1,
    },
    Submit {
        plan: OrchestrationPlanV1,
        review: PlanReviewV1,
    },
    SubmitHandoff {
        envelope: HandoffEnvelopeV1,
        confirmation: HandoffConfirmationV1,
    },
    List,
    Get {
        run_id: String,
    },
    Pause {
        run_id: String,
    },
    Resume {
        run_id: String,
    },
    Stop {
        run_id: String,
    },
    VerifyStep {
        run_id: String,
        step_id: String,
        reviewed_workspace_revision: String,
    },
    Reconcile {
        run_id: String,
        step_id: String,
        decision: ReconcileDecision,
        workspace_root: Option<String>,
        reviewed_workspace_revision: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonError {
    Unavailable,
    InvalidRequest,
    InvalidPlan,
    InvalidHandoff,
    ReviewStale,
    TrustBoundaryBlocked,
    ProviderBlocked,
    PermissionDenied,
    ResourceBusy,
    WorkspaceConflict,
    JournalUnavailable,
    AuthorityUnavailable,
    UnknownRun,
    InvalidTransition,
    Expired,
    EffectUnknown,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DaemonReply {
    value: Option<Value>,
    error: Option<DaemonError>,
}

/// Ensure the owner-requested headless process exists, then make one bounded local call.
/// Environment variables other than the three XDG path inputs are not inherited by the child.
pub fn call(request: &DaemonRequest) -> Result<Value, DaemonError> {
    let paths = Paths::from_environment().map_err(|_| DaemonError::Unavailable)?;
    let socket = paths.runtime.join(SOCKET_NAME);
    match connect_checked(&socket) {
        Ok(stream) => exchange(stream, request),
        Err(_) => {
            spawn_daemon(&paths)?;
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                if let Ok(stream) = connect_checked(&socket) {
                    return exchange(stream, request);
                }
                if Instant::now() >= deadline {
                    return Err(DaemonError::Unavailable);
                }
                thread::sleep(Duration::from_millis(35));
            }
        }
    }
}

/// Binary entry point selected only by `--wyss-orchestrator`; it never opens a desktop window.
pub fn run() -> i32 {
    match run_inner() {
        Ok(()) => 0,
        Err(()) => 1,
    }
}

fn run_inner() -> Result<(), ()> {
    let paths = Paths::from_environment().map_err(|_| ())?;
    private_dir(&paths.runtime).map_err(|_| ())?;
    private_dir(&paths.state).map_err(|_| ())?;
    let lock_path = paths.runtime.join(LOCK_NAME);
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .open(&lock_path)
        .map_err(|_| ())?;
    let metadata = lock.metadata().map_err(|_| ())?;
    if !metadata.is_file()
        || metadata.uid() != geteuid().as_raw()
        || metadata.mode() & 0o777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(());
    }
    let _lock = Flock::lock(lock, FlockArg::LockExclusiveNonblock).map_err(|_| ())?;

    let socket_path = paths.runtime.join(SOCKET_NAME);
    remove_stale_socket(&socket_path)?;
    let listener = UnixListener::bind(&socket_path).map_err(|_| ())?;
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600)).map_err(|_| ())?;
    let socket_meta = fs::symlink_metadata(&socket_path).map_err(|_| ())?;
    if !socket_meta.file_type().is_socket()
        || socket_meta.uid() != geteuid().as_raw()
        || socket_meta.mode() & 0o777 != 0o600
    {
        return Err(());
    }
    listener.set_nonblocking(true).map_err(|_| ())?;

    let database = paths.state.join(DATABASE_NAME);
    let journal = Arc::new(RunJournal::open(&database).map_err(|_| ())?);
    let runtime_root = env::var_os("XDG_RUNTIME_DIR").ok_or(())?;
    let authority = Arc::new(F8Authority::new(PathBuf::from(runtime_root)));
    let coordinator = Coordinator::new(journal.clone(), authority, now()).map_err(|_| ())?;
    let mut last_request = Instant::now();

    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                // SO_PEERCRED proves same UID only. All effects still go through the
                // pre-existing authority's leases, gates, fences and action approvals.
                if crate::control::peer(&stream).is_ok()
                    && stream
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .is_ok()
                    && stream
                        .set_write_timeout(Some(Duration::from_secs(3)))
                        .is_ok()
                    && let Ok(request) = read_request(&mut stream)
                {
                    last_request = Instant::now();
                    let reply = handle(&coordinator, &journal, request);
                    let _ = write_reply(&mut stream, &reply);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => (),
            Err(_) => return Err(()),
        }
        // Scheduler errors are represented in the run journal where an action was attempted;
        // fixed local errors do not include prompts, provider responses or filesystem paths.
        let _ = coordinator.tick(now());
        if last_request.elapsed() >= IDLE_EXIT
            && !coordinator.has_background_work().map_err(|_| ())?
        {
            drop(listener);
            let _ = fs::remove_file(&socket_path);
            return Ok(());
        }
        thread::sleep(TICK);
    }
}

fn handle(coordinator: &Coordinator, _journal: &RunJournal, request: DaemonRequest) -> DaemonReply {
    let result: Result<Value, DaemonError> = (|| match request {
        DaemonRequest::WorkspaceSnapshot { workspace_root } => {
            let root = Path::new(&workspace_root);
            let identity = workspace_identity(root).map_err(|_| DaemonError::WorkspaceConflict)?;
            let revision = workspace_revision(root).map_err(|_| DaemonError::WorkspaceConflict)?;
            serde_json::to_value(WorkspaceSnapshot {
                workspace_id: identity,
                workspace_revision: revision,
            })
            .map_err(|_| DaemonError::InvalidRequest)
        }
        DaemonRequest::ReviewPlan { plan } => {
            plan.validate(now()).map_err(map_error)?;
            let identity = workspace_identity(Path::new(&plan.workspace_root))
                .map_err(|_| DaemonError::WorkspaceConflict)?;
            let revision = workspace_revision(Path::new(&plan.workspace_root))
                .map_err(|_| DaemonError::WorkspaceConflict)?;
            if identity != plan.workspace_id || revision != plan.workspace_revision {
                return Err(DaemonError::WorkspaceConflict);
            }
            json(PlanReviewV1 {
                plan_id: plan.plan_id.clone(),
                revision: plan.revision,
                digest: plan.digest().map_err(map_error)?,
            })
        }
        DaemonRequest::SealHandoff { mut envelope } => {
            envelope.seal().map_err(map_error)?;
            envelope.validate(now()).map_err(map_error)?;
            let first_message = envelope.first_message().map_err(map_error)?;
            json(HandoffPreview {
                envelope,
                first_message,
            })
        }
        DaemonRequest::Submit { plan, review } => {
            json(coordinator.submit(plan, review, now()).map_err(map_error)?)
        }
        DaemonRequest::SubmitHandoff {
            envelope,
            confirmation,
        } => json(
            coordinator
                .submit_handoff(envelope, confirmation, now())
                .map_err(map_error)?,
        ),
        DaemonRequest::List => json(coordinator.list().map_err(map_error)?),
        DaemonRequest::Get { run_id } => json(coordinator.get(&run_id).map_err(map_error)?),
        DaemonRequest::Pause { run_id } => {
            json(coordinator.pause(&run_id, now()).map_err(map_error)?)
        }
        DaemonRequest::Resume { run_id } => {
            json(coordinator.resume(&run_id, now()).map_err(map_error)?)
        }
        DaemonRequest::Stop { run_id } => {
            json(coordinator.stop(&run_id, now()).map_err(map_error)?)
        }
        DaemonRequest::VerifyStep {
            run_id,
            step_id,
            reviewed_workspace_revision,
        } => json(
            coordinator
                .verify_step(&run_id, &step_id, &reviewed_workspace_revision, now())
                .map_err(map_error)?,
        ),
        DaemonRequest::Reconcile {
            run_id,
            step_id,
            decision,
            workspace_root,
            reviewed_workspace_revision,
        } => json(
            coordinator
                .reconcile(
                    &run_id,
                    &step_id,
                    decision,
                    workspace_root.as_deref(),
                    reviewed_workspace_revision.as_deref(),
                    now(),
                )
                .map_err(map_error)?,
        ),
    })();
    match result {
        Ok(value) => DaemonReply {
            value: Some(value),
            error: None,
        },
        Err(error) => DaemonReply {
            value: None,
            error: Some(error),
        },
    }
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceSnapshot {
    workspace_id: String,
    workspace_revision: String,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct HandoffPreview {
    envelope: HandoffEnvelopeV1,
    first_message: String,
}

fn json<T: Serialize>(value: T) -> Result<Value, DaemonError> {
    serde_json::to_value(value).map_err(|_| DaemonError::InvalidRequest)
}

fn map_error(error: CoordinatorError) -> DaemonError {
    match error {
        CoordinatorError::InvalidPlan
        | CoordinatorError::Cycle
        | CoordinatorError::ConcurrencyBlocked => DaemonError::InvalidPlan,
        CoordinatorError::InvalidHandoff => DaemonError::InvalidHandoff,
        CoordinatorError::ReviewStale => DaemonError::ReviewStale,
        CoordinatorError::TrustBoundaryBlocked => DaemonError::TrustBoundaryBlocked,
        CoordinatorError::ProviderBlocked => DaemonError::ProviderBlocked,
        CoordinatorError::SessionUnavailable | CoordinatorError::PermissionDenied => {
            DaemonError::PermissionDenied
        }
        CoordinatorError::ResourceBusy => DaemonError::ResourceBusy,
        CoordinatorError::WorkspaceConflict => DaemonError::WorkspaceConflict,
        CoordinatorError::JournalUnavailable => DaemonError::JournalUnavailable,
        CoordinatorError::AuthorityUnavailable => DaemonError::AuthorityUnavailable,
        CoordinatorError::UnknownRun => DaemonError::UnknownRun,
        CoordinatorError::InvalidTransition => DaemonError::InvalidTransition,
        CoordinatorError::Expired => DaemonError::Expired,
        CoordinatorError::EffectUnknown => DaemonError::EffectUnknown,
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn connect_checked(path: &Path) -> Result<UnixStream, DaemonError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| DaemonError::Unavailable)?;
    if !metadata.file_type().is_socket()
        || metadata.uid() != geteuid().as_raw()
        || metadata.mode() & 0o777 != 0o600
    {
        return Err(DaemonError::Unavailable);
    }
    let stream = UnixStream::connect(path).map_err(|_| DaemonError::Unavailable)?;
    crate::control::peer(&stream).map_err(|_| DaemonError::Unavailable)?;
    Ok(stream)
}

fn spawn_daemon(paths: &Paths) -> Result<(), DaemonError> {
    let executable = env::current_exe().map_err(|_| DaemonError::Unavailable)?;
    let mut command = Command::new(executable);
    command
        .arg("--wyss-orchestrator")
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    if let Some(runtime) = env::var_os("XDG_RUNTIME_DIR") {
        command.env("XDG_RUNTIME_DIR", runtime);
    }
    if let Some(state) = env::var_os("XDG_STATE_HOME") {
        command.env("XDG_STATE_HOME", state);
    }
    if let Some(home) = env::var_os("HOME") {
        command.env("HOME", home);
    }
    let _ = paths;
    command.spawn().map_err(|_| DaemonError::Unavailable)?;
    Ok(())
}

fn remove_stale_socket(path: &Path) -> Result<(), ()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(()),
        Ok(metadata) => {
            if !metadata.file_type().is_socket()
                || metadata.uid() != geteuid().as_raw()
                || metadata.mode() & 0o777 != 0o600
            {
                return Err(());
            }
            if UnixStream::connect(path).is_ok() {
                return Err(());
            }
            fs::remove_file(path).map_err(|_| ())
        }
    }
}

fn exchange(mut stream: UnixStream, request: &DaemonRequest) -> Result<Value, DaemonError> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|_| DaemonError::Unavailable)?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|_| DaemonError::Unavailable)?;
    let bytes = serde_json::to_vec(request).map_err(|_| DaemonError::InvalidRequest)?;
    if bytes.len() > MAX_REQUEST {
        return Err(DaemonError::InvalidRequest);
    }
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .map_err(|_| DaemonError::Unavailable)?;
    stream
        .write_all(&bytes)
        .map_err(|_| DaemonError::Unavailable)?;
    stream.flush().map_err(|_| DaemonError::Unavailable)?;
    let reply: DaemonReply = read_frame(&mut stream)?;
    match (reply.value, reply.error) {
        (Some(value), None) => Ok(value),
        (None, Some(error)) => Err(error),
        _ => Err(DaemonError::Unavailable),
    }
}

fn read_request(stream: &mut UnixStream) -> Result<DaemonRequest, ()> {
    let length = read_length(stream)?;
    if length == 0 || length > MAX_REQUEST {
        return Err(());
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).map_err(|_| ())?;
    serde_json::from_slice(&bytes).map_err(|_| ())
}

fn read_frame<T: for<'de> Deserialize<'de>>(stream: &mut UnixStream) -> Result<T, DaemonError> {
    let length = read_length(stream).map_err(|_| DaemonError::Unavailable)?;
    if length == 0 || length > MAX_FRAME {
        return Err(DaemonError::Unavailable);
    }
    let mut bytes = vec![0; length];
    stream
        .read_exact(&mut bytes)
        .map_err(|_| DaemonError::Unavailable)?;
    serde_json::from_slice(&bytes).map_err(|_| DaemonError::Unavailable)
}

fn read_length(stream: &mut UnixStream) -> Result<usize, ()> {
    let mut length = [0u8; 4];
    stream.read_exact(&mut length).map_err(|_| ())?;
    Ok(u32::from_be_bytes(length) as usize)
}

fn write_reply(stream: &mut UnixStream, reply: &DaemonReply) -> Result<(), ()> {
    let bytes = serde_json::to_vec(reply).map_err(|_| ())?;
    if bytes.len() > MAX_FRAME {
        return Err(());
    }
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .map_err(|_| ())?;
    stream.write_all(&bytes).map_err(|_| ())?;
    stream.flush().map_err(|_| ())
}
