//! Local human terminal resources only; no managed-agent authority or durable state.

mod buffer;
pub mod contracts;
#[cfg(test)]
mod tests;
mod worker;

use crate::contracts::identities::TerminalId;
use buffer::OutputBuffer;
use contracts::*;
use std::{
    collections::{BTreeMap, VecDeque},
    io::Write,
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const CLOSE_WAIT: Duration = Duration::from_secs(2);
const CALL_WAIT: Duration = Duration::from_secs(2);
const TOMBSTONES: usize = 128;

type Reply<T> = SyncSender<Result<T, TerminalError>>;
pub(crate) enum Command {
    Write {
        data: Vec<u8>,
        deadline: Instant,
        reply: Reply<usize>,
    },
    Resize {
        size: TerminalSize,
        deadline: Instant,
        reply: Reply<TerminalInfo>,
    },
}

pub(crate) struct Shared {
    pub data: Mutex<LocalState>,
    pub changed: Condvar,
    pub stop: AtomicBool,
}
pub(crate) struct LocalState {
    pub info: TerminalInfo,
    pub output: OutputBuffer,
    pub completed: bool,
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct Runtime {
    shared: Arc<Shared>,
    commands: SyncSender<Command>,
    wake: Mutex<UnixStream>,
    worker: Mutex<Option<JoinHandle<()>>>,
    abandoned: AtomicBool,
}
impl Runtime {
    fn wake(&self) {
        let _ = lock(&self.wake).write(&[1]);
    }
    fn stop(&self) {
        let mut state = lock(&self.shared.data);
        if !state.completed {
            state.info.state = TerminalState::Closing;
        }
        self.shared.stop.store(true, Ordering::Release);
        drop(state);
        self.shared.changed.notify_all();
        self.wake();
    }
    fn wait(&self, deadline: Instant) -> Result<TerminalInfo, TerminalError> {
        let mut state = lock(&self.shared.data);
        while !state.completed {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(TerminalError::new(ErrorCode::CloseTimeout));
            }
            state = self
                .shared
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
        let info = state.info.clone();
        drop(state);
        if let Some(handle) = lock(&self.worker).take() {
            handle
                .join()
                .map_err(|_| TerminalError::new(ErrorCode::InternalPty))?;
        }
        Ok(info)
    }
}

#[derive(Default)]
struct Registry {
    active: Mutex<BTreeMap<String, Arc<Runtime>>>,
    closed: Mutex<VecDeque<TerminalInfo>>,
    shutdown: AtomicBool,
}
impl Drop for Registry {
    fn drop(&mut self) {
        for runtime in lock(&self.active).values() {
            runtime.stop();
        }
    }
}

#[derive(Clone, Default)]
pub struct TerminalService {
    registry: Arc<Registry>,
}

fn key(id: &TerminalId) -> String {
    id.as_str().to_ascii_lowercase()
}

impl TerminalService {
    pub fn create(&self, request: CreateTerminal) -> Result<TerminalInfo, TerminalError> {
        let (shell, home) = shell_and_home()?;
        self.create_with_shell(request, shell, home)
    }

    fn create_with_shell(
        &self,
        request: CreateTerminal,
        shell: PathBuf,
        home: PathBuf,
    ) -> Result<TerminalInfo, TerminalError> {
        let launch = Launch {
            program: shell,
            args: vec!["-i".to_owned()],
            env: Vec::new(),
            remove_env: Vec::new(),
        };
        self.spawn(request, launch, home)
    }

    /// A terminal that runs `launch` instead of a bare shell (F3a). `request.cwd` must name
    /// a directory; the desktop binary has already validated it.
    pub fn create_launch(
        &self,
        request: CreateTerminal,
        launch: Launch,
    ) -> Result<TerminalInfo, TerminalError> {
        let (_, home) = shell_and_home()?;
        self.spawn(request, launch, home)
    }

    /// The user's login shell, as a bare terminal would start it.
    pub fn user_shell() -> Result<PathBuf, TerminalError> {
        Ok(shell_and_home()?.0)
    }

    fn spawn(
        &self,
        request: CreateTerminal,
        launch: Launch,
        home: PathBuf,
    ) -> Result<TerminalInfo, TerminalError> {
        let size = request.size.validate()?;
        let cwd = request.cwd.map(PathBuf::from).unwrap_or(home);
        let cwd = cwd
            .canonicalize()
            .map_err(|error| TerminalError::context(ErrorCode::InvalidCwd, error))?;
        if !cwd.is_dir() || cwd.as_os_str().len() > 4096 {
            return Err(TerminalError::new(ErrorCode::InvalidCwd));
        }
        let id = TerminalId::parse(&uuid::Uuid::new_v4().to_string())
            .map_err(|error| TerminalError::context(ErrorCode::InternalPty, error))?;
        let info = TerminalInfo {
            terminal_id: id.clone(),
            shell: launch.program.to_string_lossy().into_owned(),
            cwd: cwd.to_string_lossy().into_owned(),
            observed_pid: None,
            size,
            state: TerminalState::Starting,
            exit: None,
            error: None,
        };
        let shared = Arc::new(Shared {
            data: Mutex::new(LocalState {
                info,
                output: OutputBuffer::new(),
                completed: false,
            }),
            changed: Condvar::new(),
            stop: AtomicBool::new(false),
        });
        let (wake_reader, wake_writer) = UnixStream::pair()
            .map_err(|error| TerminalError::context(ErrorCode::InternalPty, error))?;
        wake_writer
            .set_nonblocking(true)
            .map_err(|error| TerminalError::context(ErrorCode::InternalPty, error))?;
        wake_reader
            .set_nonblocking(true)
            .map_err(|error| TerminalError::context(ErrorCode::InternalPty, error))?;
        let (commands, receiver) = mpsc::sync_channel(COMMAND_CAPACITY);
        let (boot, ready) = mpsc::sync_channel(1);
        let runtime = Arc::new(Runtime {
            shared: shared.clone(),
            commands,
            wake: Mutex::new(wake_writer),
            worker: Mutex::new(None),
            abandoned: AtomicBool::new(false),
        });
        {
            let mut active = lock(&self.registry.active);
            if self.registry.shutdown.load(Ordering::Acquire) {
                return Err(TerminalError::new(ErrorCode::AlreadyClosed));
            }
            // A close can time out while the kernel delays exit. Once its worker
            // later completes, reclaim its slot before admitting another terminal.
            let mut closed = lock(&self.registry.closed);
            active.retain(|_, resource| {
                let state = lock(&resource.shared.data);
                if state.completed && state.info.state == TerminalState::Closed {
                    closed.push_back(state.info.clone());
                    false
                } else {
                    !(resource.abandoned.load(Ordering::Acquire) && state.completed)
                }
            });
            while closed.len() > TOMBSTONES {
                closed.pop_front();
            }
            drop(closed);
            if active.len() >= MAX_TERMINALS {
                return Err(TerminalError::new(ErrorCode::LimitReached));
            }
            active.insert(key(&id), runtime.clone());
        }
        let worker = thread::Builder::new()
            .name(format!("wyss-pty-{}", id.as_str()))
            .spawn(move || worker::run(launch, cwd, size, shared, receiver, wake_reader, boot));
        match worker {
            Ok(handle) => *lock(&runtime.worker) = Some(handle),
            Err(error) => {
                lock(&self.registry.active).remove(&key(&id));
                return Err(TerminalError::context(ErrorCode::InternalPty, error));
            }
        }
        match ready.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(info)) => Ok(info),
            Ok(Err(error)) => {
                let _ = runtime.wait(Instant::now() + CLOSE_WAIT);
                lock(&self.registry.active).remove(&key(&id));
                Err(error)
            }
            Err(error) => {
                runtime.abandoned.store(true, Ordering::Release);
                runtime.stop();
                Err(TerminalError::context(ErrorCode::OperationTimeout, error))
            }
        }
    }

    fn runtime(&self, id: &TerminalId) -> Result<Arc<Runtime>, TerminalError> {
        if let Some(runtime) = lock(&self.registry.active).get(&key(id)).cloned() {
            return Ok(runtime);
        }
        if lock(&self.registry.closed)
            .iter()
            .any(|info| key(&info.terminal_id) == key(id))
        {
            return Err(TerminalError::new(ErrorCode::AlreadyClosed));
        }
        Err(TerminalError::new(ErrorCode::UnknownTerminal))
    }
    pub fn status(&self, id: &TerminalId) -> Result<TerminalInfo, TerminalError> {
        match self.runtime(id) {
            Ok(runtime) => Ok(lock(&runtime.shared.data).info.clone()),
            Err(error) if error.code == ErrorCode::AlreadyClosed => lock(&self.registry.closed)
                .iter()
                .find(|info| key(&info.terminal_id) == key(id))
                .cloned()
                .ok_or(error),
            Err(error) => Err(error),
        }
    }
    fn running(runtime: &Runtime) -> Result<(), TerminalError> {
        if lock(&runtime.shared.data).info.state != TerminalState::Running {
            return Err(TerminalError::new(ErrorCode::AlreadyClosed));
        }
        Ok(())
    }
    pub fn write(&self, id: &TerminalId, data: Vec<u8>) -> Result<usize, TerminalError> {
        let runtime = self.runtime(id)?;
        Self::running(&runtime)?;
        if data.is_empty() || data.len() > INPUT_LIMIT {
            return Err(TerminalError::new(ErrorCode::InvalidInput));
        }
        let (reply, result) = mpsc::sync_channel(1);
        runtime
            .commands
            .try_send(Command::Write {
                data,
                deadline: Instant::now() + Duration::from_secs(1),
                reply,
            })
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => TerminalError::new(ErrorCode::Busy),
                mpsc::TrySendError::Disconnected(_) => TerminalError::new(ErrorCode::AlreadyClosed),
            })?;
        runtime.wake();
        result
            .recv_timeout(CALL_WAIT)
            .map_err(|error| TerminalError::context(ErrorCode::OperationTimeout, error))?
    }
    pub fn resize(
        &self,
        id: &TerminalId,
        size: TerminalSize,
    ) -> Result<TerminalInfo, TerminalError> {
        let runtime = self.runtime(id)?;
        Self::running(&runtime)?;
        let size = size.validate()?;
        let (reply, result) = mpsc::sync_channel(1);
        runtime
            .commands
            .try_send(Command::Resize {
                size,
                deadline: Instant::now() + Duration::from_secs(1),
                reply,
            })
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => TerminalError::new(ErrorCode::Busy),
                mpsc::TrySendError::Disconnected(_) => TerminalError::new(ErrorCode::AlreadyClosed),
            })?;
        runtime.wake();
        result
            .recv_timeout(CALL_WAIT)
            .map_err(|error| TerminalError::context(ErrorCode::OperationTimeout, error))?
    }
    pub fn read(
        &self,
        id: &TerminalId,
        ack: Option<&crate::contracts::WireUuid>,
    ) -> Result<TerminalOutput, TerminalError> {
        let runtime = self.runtime(id)?;
        let mut state = lock(&runtime.shared.data);
        if matches!(
            state.info.state,
            TerminalState::Closing | TerminalState::Closed
        ) {
            return Err(TerminalError::new(ErrorCode::AlreadyClosed));
        }
        let (batch_id, data) = state.output.take(ack)?;
        let output = TerminalOutput {
            batch_id,
            data,
            eof: state.output.eof,
            truncated: state.output.truncated,
            status: state.info.clone(),
        };
        drop(state);
        runtime.wake();
        Ok(output)
    }
    pub fn close(&self, id: &TerminalId) -> Result<TerminalInfo, TerminalError> {
        let runtime = self.runtime(id)?;
        {
            let mut state = lock(&runtime.shared.data);
            if matches!(
                state.info.state,
                TerminalState::Closing | TerminalState::Closed
            ) {
                return Err(TerminalError::new(ErrorCode::AlreadyClosed));
            }
            state.info.state = TerminalState::Closing;
            runtime.shared.stop.store(true, Ordering::Release);
        }
        runtime.wake();
        let mut info = runtime.wait(Instant::now() + CLOSE_WAIT)?;
        info.state = TerminalState::Closed;
        {
            let mut state = lock(&runtime.shared.data);
            state.info.state = TerminalState::Closed;
            state.output.discard();
        }
        // Keep lock ordering uniform: active before tombstones.
        let mut active = lock(&self.registry.active);
        active.remove(&key(id));
        let mut closed = lock(&self.registry.closed);
        closed.push_back(info.clone());
        if closed.len() > TOMBSTONES {
            closed.pop_front();
        }
        Ok(info)
    }
    pub fn shutdown(&self) -> Result<(), TerminalError> {
        self.registry.shutdown.store(true, Ordering::Release);
        let terminals: Vec<_> = lock(&self.registry.active).values().cloned().collect();
        for terminal in &terminals {
            terminal.stop();
        }
        let deadline = Instant::now() + CLOSE_WAIT;
        for terminal in terminals {
            terminal.wait(deadline)?;
        }
        lock(&self.registry.active).clear();
        Ok(())
    }
}

fn shell_and_home() -> Result<(PathBuf, PathBuf), TerminalError> {
    use nix::unistd::{User, getuid};
    let user = User::from_uid(getuid())
        .map_err(|error| TerminalError::context(ErrorCode::SpawnFailed, error))?;
    let shell = user
        .as_ref()
        .map(|user| user.shell.clone())
        .filter(|shell| executable(shell))
        .or_else(|| {
            std::env::var_os("SHELL")
                .map(PathBuf::from)
                .filter(|shell| executable(shell))
        })
        .unwrap_or_else(|| PathBuf::from("/bin/sh"));
    if !executable(&shell) {
        return Err(TerminalError::new(ErrorCode::SpawnFailed));
    }
    let home = user
        .map(|user| user.dir)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .ok_or_else(|| TerminalError::new(ErrorCode::InvalidCwd))?;
    Ok((shell, home))
}
fn executable(path: &std::path::Path) -> bool {
    path.is_absolute()
        && path
            .metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}
