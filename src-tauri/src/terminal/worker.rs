use super::{Command, Reply, Shared, contracts::*, lock};
use nix::{
    errno::Errno,
    fcntl::{FcntlArg, OFlag, fcntl},
    poll::{PollFd, PollFlags, poll},
    sys::signal::{Signal, kill, killpg},
    unistd::Pid,
};
use portable_pty::{Child, CommandBuilder, MasterPty, native_pty_system};
use std::{
    io::{Read, Write},
    os::{
        fd::{AsFd, BorrowedFd, OwnedFd},
        unix::net::UnixStream,
    },
    path::PathBuf,
    sync::{
        Arc,
        atomic::Ordering,
        mpsc::{Receiver, SyncSender},
    },
    time::{Duration, Instant},
};

struct IoHandles {
    master: Box<dyn MasterPty + Send>,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    fd: OwnedFd,
}

/// Owns the unreaped child. Last-resort unwinding cleanup runs only on its worker.
struct ProcessGuard {
    child: Option<Box<dyn Child + Send + Sync>>,
}
impl Drop for ProcessGuard {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            if let Some(pid) = child.process_id() {
                let _ = kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
            }
            let _ = child.wait();
        }
    }
}

fn open(
    shell: PathBuf,
    cwd: PathBuf,
    size: TerminalSize,
) -> Result<(IoHandles, ProcessGuard), TerminalError> {
    let pair = native_pty_system()
        .openpty(size.pty())
        .map_err(|error| TerminalError::context(ErrorCode::InternalPty, error))?;
    let fd = pair
        .master
        .as_raw_fd()
        .ok_or_else(|| TerminalError::new(ErrorCode::InternalPty))?;
    // SAFETY: master owns this live fd. Duplicate it immediately while master is in scope;
    // OwnedFd then provides a checked lifetime for poll, independently of trait objects.
    let owned = unsafe { BorrowedFd::borrow_raw(fd) }
        .try_clone_to_owned()
        .map_err(|error| TerminalError::context(ErrorCode::InternalPty, error))?;
    let flags = fcntl(fd, FcntlArg::F_GETFL)
        .map_err(|error| TerminalError::context(ErrorCode::InternalPty, error))?;
    fcntl(
        fd,
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
    )
    .map_err(|error| TerminalError::context(ErrorCode::InternalPty, error))?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| TerminalError::context(ErrorCode::InternalPty, error))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| TerminalError::context(ErrorCode::InternalPty, error))?;
    let mut command = CommandBuilder::new(shell);
    command.arg("-i");
    command.cwd(cwd);
    command.env("TERM", "dumb");
    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| TerminalError::context(ErrorCode::SpawnFailed, error))?;
    drop(pair.slave);
    Ok((
        IoHandles {
            master: pair.master,
            reader,
            writer,
            fd: owned,
        },
        ProcessGuard { child: Some(child) },
    ))
}

struct PendingWrite {
    data: Vec<u8>,
    offset: usize,
    deadline: Instant,
    reply: Reply<usize>,
}

fn reply<T>(sender: Reply<T>, value: Result<T, TerminalError>) {
    let _ = sender.try_send(value);
}

fn stop_write(pending: &mut Option<PendingWrite>, code: ErrorCode) {
    if let Some(write) = pending.take() {
        reply(
            write.reply,
            Err(TerminalError::new(code).partial(write.offset)),
        );
    }
}

fn drain_commands(
    receiver: &Receiver<Command>,
    pending: &mut Option<PendingWrite>,
    io: &IoHandles,
    shared: &Shared,
) {
    // A finite budget prevents a stream of control requests from starving exit/close.
    for _ in 0..COMMAND_CAPACITY {
        let Ok(command) = receiver.try_recv() else {
            break;
        };
        match command {
            Command::Write {
                data,
                deadline,
                reply: sender,
            } => {
                if shared.stop.load(Ordering::Acquire)
                    || lock(&shared.data).info.state != TerminalState::Running
                {
                    reply(sender, Err(TerminalError::new(ErrorCode::AlreadyClosed)));
                } else if Instant::now() >= deadline {
                    reply(sender, Err(TerminalError::new(ErrorCode::OperationTimeout)));
                } else if pending.is_some() {
                    reply(sender, Err(TerminalError::new(ErrorCode::Busy)));
                } else {
                    *pending = Some(PendingWrite {
                        data,
                        offset: 0,
                        deadline,
                        reply: sender,
                    });
                }
            }
            Command::Resize {
                size,
                deadline,
                reply: sender,
            } => {
                if shared.stop.load(Ordering::Acquire)
                    || lock(&shared.data).info.state != TerminalState::Running
                {
                    reply(sender, Err(TerminalError::new(ErrorCode::AlreadyClosed)));
                } else if Instant::now() >= deadline {
                    reply(sender, Err(TerminalError::new(ErrorCode::OperationTimeout)));
                } else {
                    let result = io
                        .master
                        .resize(size.pty())
                        .map_err(|error| TerminalError::context(ErrorCode::ResizeFailed, error))
                        .map(|()| {
                            let mut state = lock(&shared.data);
                            state.info.size = size;
                            state.info.clone()
                        });
                    reply(sender, result);
                }
            }
        }
    }
}

fn reject_commands(receiver: &Receiver<Command>) {
    while let Ok(command) = receiver.try_recv() {
        match command {
            Command::Write { reply: sender, .. } => {
                reply(sender, Err(TerminalError::new(ErrorCode::AlreadyClosed)))
            }
            Command::Resize { reply: sender, .. } => {
                reply(sender, Err(TerminalError::new(ErrorCode::AlreadyClosed)))
            }
        }
    }
}

fn read_once(io: &mut IoHandles, shared: &Shared) -> Result<bool, TerminalError> {
    let remaining = lock(&shared.data).output.remaining().min(4096);
    if remaining == 0 {
        return Ok(false);
    }
    let mut bytes = [0; 4096];
    match io.reader.read(&mut bytes[..remaining]) {
        Ok(0) => {
            lock(&shared.data).output.eof = true;
            Ok(false)
        }
        Ok(count) => {
            lock(&shared.data).output.append(&bytes[..count]);
            shared.changed.notify_all();
            Ok(true)
        }
        Err(error)
            if error.kind() == std::io::ErrorKind::WouldBlock
                || error.kind() == std::io::ErrorKind::Interrupted =>
        {
            Ok(false)
        }
        Err(error) if error.raw_os_error() == Some(Errno::EIO as i32) => {
            lock(&shared.data).output.eof = true;
            Ok(false)
        }
        Err(error) => Err(TerminalError::context(ErrorCode::InternalPty, error)),
    }
}

fn signal_group(group: Option<i32>, signal: Signal) {
    if let Some(group) = group.filter(|group| *group > 0) {
        let _ = killpg(Pid::from_raw(group), signal);
    }
}

pub(crate) fn run(
    shell: PathBuf,
    cwd: PathBuf,
    size: TerminalSize,
    shared: Arc<Shared>,
    receiver: Receiver<Command>,
    mut wake: UnixStream,
    boot: SyncSender<Result<TerminalInfo, TerminalError>>,
) {
    let opened = open(shell, cwd, size);
    let (handles, mut process) = match opened {
        Ok(resources) => resources,
        Err(error) => {
            let mut state = lock(&shared.data);
            state.info.state = TerminalState::Failed;
            state.info.error = Some(error.code);
            state.output.eof = true;
            state.completed = true;
            drop(state);
            shared.changed.notify_all();
            reply(boot, Err(error));
            return;
        }
    };
    {
        let mut state = lock(&shared.data);
        state.info.observed_pid = process.child.as_ref().and_then(|child| child.process_id());
        if !shared.stop.load(Ordering::Acquire) {
            state.info.state = TerminalState::Running;
        }
        reply(boot, Ok(state.info.clone()));
    }
    let shell_pid = process
        .child
        .as_ref()
        .and_then(|child| child.process_id())
        .map(|pid| pid as i32);
    let mut io = Some(handles);
    let mut pending = None;
    let mut closing: Option<Instant> = None;
    let mut foreground = None;
    let mut escalated = false;
    let mut failure = None;

    loop {
        if shared.stop.load(Ordering::Acquire) && closing.is_none() {
            closing = Some(Instant::now());
            stop_write(&mut pending, ErrorCode::AlreadyClosed);
            if let Some(handles) = io.take() {
                foreground = handles.master.process_group_leader();
                signal_group(foreground, Signal::SIGHUP);
                signal_group(shell_pid, Signal::SIGHUP);
                // Dropping every master descriptor and reader/writer releases the PTY now.
                drop(handles);
            }
            reject_commands(&receiver);
        }
        let status = process
            .child
            .as_mut()
            .expect("worker owns unreaped child")
            .try_wait();
        match status {
            Ok(Some(exit)) => {
                process.child = None; // already reaped by try_wait; guard must not signal a reused PID.
                stop_write(&mut pending, ErrorCode::AlreadyClosed);
                reject_commands(&receiver);
                if let Some(mut handles) = io.take() {
                    // Natural exit: drain only remaining bounded capacity, never await a consumer.
                    for _ in 0..(OUTPUT_CAPACITY / 4096) {
                        if shared.stop.load(Ordering::Acquire)
                            || lock(&shared.data).output.remaining() == 0
                        {
                            break;
                        }
                        match read_once(&mut handles, &shared) {
                            Ok(true) => {}
                            Ok(false) => break,
                            Err(error) => {
                                failure = Some(error.code);
                                break;
                            }
                        }
                    }
                    // Probe once, including when a concurrent consumer freed capacity:
                    // exit draining has a fixed read budget, not just a queue bound.
                    let mut probe = [0; 1];
                    if handles.reader.read(&mut probe).is_ok_and(|count| count > 0) {
                        lock(&shared.data).output.truncated = true;
                    }
                    drop(handles);
                }
                let close_requested = closing.is_some() || shared.stop.load(Ordering::Acquire);
                let mut state = lock(&shared.data);
                state.info.exit = Some(ProcessExit {
                    code: exit.exit_code(),
                    signal: exit.signal().map(str::to_owned),
                });
                state.info.error = failure;
                state.info.state = if failure.is_some() {
                    TerminalState::Failed
                } else if close_requested {
                    TerminalState::Closed
                } else {
                    TerminalState::Exited
                };
                state.output.eof = true;
                if close_requested {
                    state.output.discard();
                }
                state.completed = true;
                drop(state);
                shared.changed.notify_all();
                break;
            }
            Ok(None) => {}
            Err(error) if error.raw_os_error() == Some(Errno::ECHILD as i32) => {
                // Someone else has reaped the child. Never signal its cached PID:
                // it is no longer reserved by our process and could be reused.
                process.child = None;
                drop(io.take());
                stop_write(&mut pending, ErrorCode::InternalPty);
                reject_commands(&receiver);
                let mut state = lock(&shared.data);
                state.info.state = TerminalState::Failed;
                state.info.error = Some(ErrorCode::InternalPty);
                state.output.eof = true;
                state.output.truncated = true;
                state.completed = true;
                drop(state);
                shared.changed.notify_all();
                break;
            }
            Err(error) => {
                failure = Some(ErrorCode::InternalPty);
                // Preserve the process handle and retry reaping during shutdown.
                let _diagnostic = error;
                shared.stop.store(true, Ordering::Release);
            }
        }
        if closing.is_some_and(|started| started.elapsed() >= Duration::from_millis(500))
            && !escalated
        {
            signal_group(foreground, Signal::SIGKILL);
            signal_group(shell_pid, Signal::SIGKILL);
            if let Some(pid) = shell_pid {
                let _ = kill(Pid::from_raw(pid), Signal::SIGKILL);
            }
            escalated = true;
        }
        if let Some(handles) = io.as_mut() {
            drain_commands(&receiver, &mut pending, handles, &shared);
            if pending
                .as_ref()
                .is_some_and(|write| Instant::now() >= write.deadline)
            {
                stop_write(&mut pending, ErrorCode::WriteFailed);
            }
            if let Some(write) = pending.as_mut() {
                match handles.writer.write(&write.data[write.offset..]) {
                    Ok(0) => stop_write(&mut pending, ErrorCode::WriteFailed),
                    Ok(count) => {
                        write.offset += count;
                        if write.offset == write.data.len() {
                            let write = pending.take().unwrap();
                            reply(write.reply, Ok(write.offset));
                        }
                    }
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            || error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => {
                        let write = pending.take().unwrap();
                        reply(
                            write.reply,
                            Err(TerminalError::context(ErrorCode::WriteFailed, error)
                                .partial(write.offset)),
                        );
                    }
                }
            }
        }
        let mut flags = PollFlags::empty();
        {
            let state = lock(&shared.data);
            if state.output.remaining() > 0 && !state.output.eof {
                flags |= PollFlags::POLLIN;
            }
        }
        if pending.is_some() {
            flags |= PollFlags::POLLOUT;
        }
        let mut descriptors = vec![PollFd::new(wake.as_fd(), PollFlags::POLLIN)];
        if let Some(handles) = io.as_ref().filter(|_| !flags.is_empty()) {
            descriptors.push(PollFd::new(handles.fd.as_fd(), flags));
        }
        match poll(&mut descriptors, 100_u16) {
            Ok(_) => {
                let output_ready = descriptors
                    .get(1)
                    .and_then(|descriptor| descriptor.revents())
                    .is_some_and(|events| {
                        events
                            .intersects(PollFlags::POLLIN | PollFlags::POLLHUP | PollFlags::POLLERR)
                    });
                let wake_ready = descriptors[0].revents().is_some_and(|events| {
                    events.intersects(PollFlags::POLLIN | PollFlags::POLLHUP)
                });
                drop(descriptors);
                if wake_ready {
                    let mut bytes = [0; 64];
                    for _ in 0..4 {
                        if !wake.read(&mut bytes).is_ok_and(|count| count > 0) {
                            break;
                        }
                    }
                }
                if output_ready
                    && !shared.stop.load(Ordering::Acquire)
                    && let Some(handles) = io.as_mut()
                    && let Err(error) = read_once(handles, &shared)
                {
                    failure = Some(error.code);
                    shared.stop.store(true, Ordering::Release);
                }
            }
            Err(Errno::EINTR) => {}
            Err(_) => {
                failure = Some(ErrorCode::InternalPty);
                shared.stop.store(true, Ordering::Release);
            }
        }
    }
}
