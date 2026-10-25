//! The authority's own dispatch of an approved action.
//!
//! The Claude profile offers the model no tool that can act: `run_command` and `write_file` are
//! proposals. When an approval is accepted, **this module** performs the effect, not the
//! provider. That is what makes "the provider cannot execute an effect through an unmediated
//! route" a property of the design rather than of a configuration:
//!
//! - a command runs in a fresh bubblewrap namespace of its own, with no network, an empty
//!   environment, a private `/tmp`, the workspace as its only writable path, and the names the
//!   runtime protects (`.git`, `.claude`, `.codex`, `.agents`) bound read-only; a runner inside
//!   it bounds the output and the time;
//! - a file is written by the authority: the path is confined again at the moment of the write,
//!   the content the user saw is the content written, the file must still be exactly what the
//!   preview was computed against (a stale approval writes nothing), and the replacement is
//!   atomic.
//!
//! A dispatch is started at most once; whether it *finished* is a separate fact. If the authority
//! cannot say whether an effect happened (the runner died, the deadline passed) the outcome is
//! `Indeterminate` and nothing is ever retried.
use super::sandbox::{Sandbox, path_is_plain};
use crate::{
    contracts::control::{ControlError as E, ControlResult},
    control::approval_action::{Leaf, confine},
    f8::{
        engine::{StopState, Worker},
        ownership::{ContainmentPolicy, Grace},
        process_worker::DeferredProcessWorker,
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, Instant},
};

pub const MAX_OUTPUT: usize = 16 * 1024;
/// The largest existing file a write will compare itself against.
const MAX_WRITE_TARGET: usize = 16 * 1024 * 1024;
/// Where `write_file_with` is, for a test that has to act at that instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteStep {
    AfterFirstHash,
    AfterTemp,
    BeforeRename,
}
/// What the runner is told; the authority's own deadline is longer, so a runner that is stuck
/// is distinguished from a command that was merely slow.
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);
const GRACE_AFTER_TIMEOUT: Duration = Duration::from_secs(10);
const PROTECTED: [&str; 4] = [".git", ".claude", ".codex", ".agents"];

/// Whether `path` (under `workspace`) passes through a name the runtime protects. The shared
/// `confine` protects three of these; `.claude` holds project settings and hooks that Claude
/// Code itself would later run, so a write there is planting code, and is refused here too.
pub fn touches_protected(workspace: &str, path: &str) -> bool {
    Path::new(path)
        .strip_prefix(workspace)
        .map(|rel| {
            rel.components()
                .any(|c| PROTECTED.iter().any(|p| c.as_os_str() == *p))
        })
        .unwrap_or(true)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedCommand {
    pub command: String,
    pub cwd: String,
    pub workspace: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedWrite {
    pub path: String,
    pub workspace: String,
    /// Exactly the text the user approved.
    pub content: String,
    /// SHA-256 of the file the preview was computed against; `None` means the file must not
    /// exist.
    pub expected_old: Option<[u8; 32]>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Approved {
    Command(ApprovedCommand),
    Write(ApprovedWrite),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The effect was performed. For a command this says nothing about its exit status, which is
    /// part of the text.
    Completed(String),
    /// The effect did not happen (refused before any change).
    Failed(String),
    /// The authority cannot say.
    Indeterminate,
}

pub trait ActionExecutor: Send {
    /// Starts the dispatch. `Err` means nothing was started.
    fn dispatch(&mut self, action: Approved) -> ControlResult<()>;
    fn poll(&mut self, now: Instant) -> Option<Outcome>;
    fn busy(&self) -> bool;
    fn begin_stop(&mut self, now: Instant);
    fn poll_stop(&mut self, now: Instant) -> StopState;
}

/// Runs inside the namespace. Reads one JSON request line, runs the command with a shell, keeps
/// at most `limit` bytes of its combined output, kills the process group at the deadline and
/// prints exactly one JSON line.
const RUNNER: &str = r#"
import json, os, select, signal, subprocess, sys, time
req = json.loads(sys.stdin.readline())
limit, deadline = req["limit"], time.monotonic() + req["timeout_s"]
p = subprocess.Popen(["/bin/sh", "-c", req["command"]], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                     stderr=subprocess.STDOUT, start_new_session=True)
fd, buf, total, timed_out = p.stdout.fileno(), bytearray(), 0, False
while True:
    left = deadline - time.monotonic()
    if left <= 0:
        timed_out = True
        break
    r, _, _ = select.select([fd], [], [], min(left, 0.2))
    if r:
        chunk = os.read(fd, 65536)
        if not chunk:
            break
        total += len(chunk)
        if len(buf) < limit:
            buf += chunk[: limit - len(buf)]
    elif p.poll() is not None:
        # exited; drain what is left without waiting
        while select.select([fd], [], [], 0)[0]:
            chunk = os.read(fd, 65536)
            if not chunk:
                break
            total += len(chunk)
            if len(buf) < limit:
                buf += chunk[: limit - len(buf)]
        break
if timed_out:
    try:
        os.killpg(p.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
rc = p.wait()
print(json.dumps({"exit": rc if rc >= 0 else None, "signal": -rc if rc < 0 else None, "timed_out": timed_out,
                  "truncated": total > limit, "output": bytes(buf).decode("utf-8", "replace")}), flush=True)
"#;

struct Run {
    worker: DeferredProcessWorker,
    started: Instant,
    deadline: Instant,
    stopping: bool,
}

pub struct SandboxExecutor {
    run: Option<Run>,
    done: Option<Outcome>,
    timeout: Duration,
    pending_stop: bool,
}
impl Default for SandboxExecutor {
    fn default() -> Self {
        Self::new()
    }
}
impl SandboxExecutor {
    pub fn new() -> Self {
        Self {
            run: None,
            done: None,
            timeout: COMMAND_TIMEOUT,
            pending_stop: false,
        }
    }
    #[cfg(test)]
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            timeout,
            ..Self::new()
        }
    }

    fn start_command(&mut self, c: ApprovedCommand) -> ControlResult<()> {
        let ws = Path::new(&c.workspace);
        let cwd = Path::new(&c.cwd);
        if !path_is_plain(ws)
            || !path_is_plain(cwd)
            || !confine(&c.workspace, &c.cwd, Leaf::Directory)
        {
            return Err(E::GateDenied);
        }
        let mut sb = Sandbox::new(false);
        sb.bind(ws, ws);
        // The names the runtime protects stay read-only even inside the writable workspace.
        for name in PROTECTED {
            let p = ws.join(name);
            if fs::symlink_metadata(&p).is_ok_and(|m| !m.file_type().is_symlink()) {
                sb.ro_bind(&p, &p);
            }
        }
        sb.chdir(cwd)
            .setenv("PATH", "/usr/bin:/bin")
            .setenv("HOME", "/tmp")
            .setenv("LANG", "C.UTF-8")
            .setenv("TMPDIR", "/tmp");
        let command = sb.command(
            "/usr/bin/python3",
            &["-I".into(), "-c".into(), RUNNER.into()],
        );
        let mut worker = DeferredProcessWorker::spawn(
            command,
            Grace::default(),
            ContainmentPolicy::DescendantsContained,
        )?;
        let req = json!({"command": c.command, "timeout_s": self.timeout.as_secs_f64(), "limit": MAX_OUTPUT});
        let frame = serde_json::to_vec(&req).map_err(|_| E::InvalidRequest)?;
        if let Err(e) = worker.send(&frame) {
            // Nothing was written to a command's stdin that could have run it.
            worker.begin_stop(Instant::now());
            self.run = Some(Run {
                worker,
                started: Instant::now(),
                deadline: Instant::now(),
                stopping: true,
            });
            self.pending_stop = true;
            return Err(e);
        }
        let now = Instant::now();
        self.run = Some(Run {
            worker,
            started: now,
            deadline: now + self.timeout + GRACE_AFTER_TIMEOUT,
            stopping: false,
        });
        Ok(())
    }

    fn write_file(w: ApprovedWrite) -> Outcome {
        Self::write_file_with(w, &mut |_| ())
    }

    /// The write, with `hook` told where it is, so a test can change the world at exactly the
    /// point a concurrent writer would.
    ///
    /// The directories above the file are resolved **once, by descriptor, without following a link**,
    /// and every later step is relative to the last descriptor: replacing a parent with a link or
    /// moving it cannot redirect the write. The file is hashed, the replacement is prepared next to
    /// it (on the descriptor it opened, non-blocking), and the file, the replacement's identity and the
    /// directory's place are all checked again immediately before an atomic `renameat`. What is *not*
    /// excluded: a different writer changing the file itself, swapping the replacement or moving the
    /// directory in the instant between those last checks and the rename; the previous contents are
    /// not kept. A writer that is the same user and can write the directory can always do the same
    /// without any of this, so this narrows a window and does not claim to close it.
    fn write_file_with(w: ApprovedWrite, hook: &mut dyn FnMut(WriteStep)) -> Outcome {
        use nix::{
            fcntl::{OFlag, openat, renameat},
            sys::stat::{Mode, SFlag, fstat, fstatat},
            unistd::{UnlinkatFlags, unlinkat},
        };
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
        let fail = |m: &str| Outcome::Failed(m.to_owned());
        if !path_is_plain(Path::new(&w.path))
            || touches_protected(&w.workspace, &w.path)
            || !confine(&w.workspace, &w.path, Leaf::FileOrMissing)
        {
            return fail("the path is no longer inside the workspace");
        }
        let Ok(relative) = Path::new(&w.path).strip_prefix(&w.workspace) else {
            return fail("the path is no longer inside the workspace");
        };
        let Some(names) = relative
            .components()
            .map(|c| match c {
                std::path::Component::Normal(n) => Some(n),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
        else {
            return fail("the path is no longer inside the workspace");
        };
        let Some((leaf, parents)) = names.split_last() else {
            return fail("the path is no longer inside the workspace");
        };
        let directory = OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
        // The workspace, then each parent, by descriptor and without following a link.
        let Ok(root) = nix::fcntl::open(Path::new(&w.workspace), directory, Mode::empty()) else {
            return fail("the workspace cannot be opened");
        };
        // SAFETY: `open` just returned this descriptor and nothing else owns it.
        let mut dir = unsafe { OwnedFd::from_raw_fd(root) };
        let Ok(root_stat) = fstat(dir.as_raw_fd()) else {
            return fail("the workspace cannot be inspected");
        };
        for name in parents {
            let Ok(next) = openat(Some(dir.as_raw_fd()), *name, directory, Mode::empty()) else {
                return fail("a directory above the file is not a plain directory");
            };
            // SAFETY: as above.
            let next = unsafe { OwnedFd::from_raw_fd(next) };
            match fstat(next.as_raw_fd()) {
                Ok(m) if m.st_dev == root_stat.st_dev => dir = next,
                _ => return fail("a directory above the file is on another filesystem"),
            }
        }
        let dirfd = dir.as_raw_fd();
        let Ok(dir_stat) = fstat(dirfd) else {
            return fail("the directory cannot be inspected");
        };
        // What is at the target now, judged on the descriptor that was opened and not on a name that
        // may since have changed: `Ok(None)` when nothing is there, `Err` when it is not a plain file.
        // The open is non-blocking, so a FIFO put there cannot hold this thread.
        let read_hash = || -> Result<Option<([u8; 32], u32)>, &'static str> {
            let fd = match openat(
                Some(dirfd),
                *leaf,
                OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
                Mode::empty(),
            ) {
                Err(nix::errno::Errno::ENOENT) => return Ok(None),
                Err(_) => return Err("the target is not a plain file"),
                Ok(fd) => fd,
            };
            // SAFETY: as above.
            let mut file = fs::File::from(unsafe { OwnedFd::from_raw_fd(fd) });
            let m = fstat(file.as_raw_fd()).map_err(|_| "the target cannot be inspected")?;
            if SFlag::from_bits_truncate(m.st_mode) & SFlag::S_IFMT != SFlag::S_IFREG
                || m.st_nlink != 1
            {
                return Err("the target is not a plain file");
            }
            let mut hasher = Sha256::new();
            let mut buffer = [0u8; 65536];
            let mut total = 0usize;
            loop {
                let n = std::io::Read::read(&mut file, &mut buffer)
                    .map_err(|_| "the target cannot be read")?;
                if n == 0 {
                    break;
                }
                total += n;
                if total > MAX_WRITE_TARGET {
                    return Err("the target is too large to compare");
                }
                hasher.update(&buffer[..n]);
            }
            Ok(Some((
                <[u8; 32]>::from(hasher.finalize()),
                m.st_mode & 0o777,
            )))
        };
        let before = match read_hash() {
            Ok(h) => h,
            Err(m) => return fail(m),
        };
        if before.map(|(h, _)| h) != w.expected_old {
            return fail("the file changed after the preview was made; nothing was written");
        }
        hook(WriteStep::AfterFirstHash);
        let mode = before.map_or(0o644, |(_, mode)| mode);
        let tmp = std::ffi::OsString::from(format!(".wyss-write-{:016x}", rand_u64()));
        let discard = |why: &str| {
            let _ = unlinkat(Some(dirfd), tmp.as_os_str(), UnlinkatFlags::NoRemoveDir);
            fail(why)
        };
        // The replacement is prepared on a descriptor that stays open until it is published: the
        // permissions are set on it (never on a name), and its identity is what is checked later.
        let prepared = (|| -> std::io::Result<(fs::File, nix::sys::stat::FileStat)> {
            let fd = openat(
                Some(dirfd),
                tmp.as_os_str(),
                OFlag::O_WRONLY
                    | OFlag::O_CREAT
                    | OFlag::O_EXCL
                    | OFlag::O_NOFOLLOW
                    | OFlag::O_CLOEXEC,
                Mode::from_bits_truncate(0o600),
            )?;
            // SAFETY: as above.
            let mut f = fs::File::from(unsafe { OwnedFd::from_raw_fd(fd) });
            f.write_all(w.content.as_bytes())?;
            f.sync_all()?;
            f.set_permissions(fs::Permissions::from_mode(mode))?;
            let made = fstat(f.as_raw_fd())?;
            Ok((f, made))
        })();
        let Ok((_held, made)) = prepared else {
            return discard("the file could not be prepared; nothing was written");
        };
        hook(WriteStep::AfterTemp);
        // The proof is repeated at the effect, on the same directory descriptor.
        match read_hash() {
            Ok(h) if h.map(|(h, _)| h) == w.expected_old => (),
            Ok(_) => {
                return discard(
                    "the file changed while the write was prepared; nothing was written",
                );
            }
            Err(m) => return discard(m),
        }
        // The replacement is still the file that was prepared, under the name it was prepared under.
        match fstatat(
            Some(dirfd),
            tmp.as_os_str(),
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        ) {
            Ok(now)
                if now.st_dev == made.st_dev
                    && now.st_ino == made.st_ino
                    && now.st_nlink == 1
                    && SFlag::from_bits_truncate(now.st_mode) & SFlag::S_IFMT == SFlag::S_IFREG
                    && now.st_size == w.content.len() as i64 => {}
            _ => {
                return discard(
                    "the replacement was changed before it was published; nothing was written",
                );
            }
        }
        // The directory held is still the one that lies at this place in the workspace: a directory
        // moved elsewhere (out of the workspace, say) while this ran is not written into.
        let place = || -> Option<()> {
            let root = nix::fcntl::open(Path::new(&w.workspace), directory, Mode::empty()).ok()?;
            // SAFETY: as above.
            let mut walk = unsafe { OwnedFd::from_raw_fd(root) };
            let now = fstat(walk.as_raw_fd()).ok()?;
            if (now.st_dev, now.st_ino) != (root_stat.st_dev, root_stat.st_ino) {
                return None;
            }
            for name in parents {
                let next = openat(Some(walk.as_raw_fd()), *name, directory, Mode::empty()).ok()?;
                // SAFETY: as above.
                walk = unsafe { OwnedFd::from_raw_fd(next) };
            }
            let here = fstat(walk.as_raw_fd()).ok()?;
            ((here.st_dev, here.st_ino) == (dir_stat.st_dev, dir_stat.st_ino)).then_some(())
        };
        if place().is_none() {
            return discard("the directory is no longer where it was; nothing was written");
        }
        hook(WriteStep::BeforeRename);
        match renameat(Some(dirfd), tmp.as_os_str(), Some(dirfd), *leaf) {
            Ok(()) => Outcome::Completed(format!("wrote {} bytes to {}", w.content.len(), w.path)),
            Err(_) => discard("the file could not be replaced; nothing was written"),
        }
    }

    fn result_text(v: &Value) -> Option<String> {
        let output = v.get("output")?.as_str()?;
        let truncated = v.get("truncated")?.as_bool()?;
        let timed_out = v.get("timed_out")?.as_bool()?;
        let status = if timed_out {
            "timed out and was killed".to_owned()
        } else if let Some(code) = v.get("exit").and_then(Value::as_i64) {
            format!("exit status {code}")
        } else {
            format!(
                "killed by signal {}",
                v.get("signal").and_then(Value::as_i64)?
            )
        };
        Some(format!(
            "{status}{}\n{output}",
            if truncated { " (output truncated)" } else { "" }
        ))
    }
}

fn rand_u64() -> u64 {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).expect("OS random source");
    u64::from_le_bytes(b)
}

impl ActionExecutor for SandboxExecutor {
    fn dispatch(&mut self, action: Approved) -> ControlResult<()> {
        if self.run.is_some() {
            return Err(E::ActiveTurnExists);
        }
        match action {
            Approved::Command(c) => self.start_command(c),
            Approved::Write(w) => {
                // A write finishes inside the call; its outcome is delivered by the next `poll`.
                let outcome = Self::write_file(w);
                self.done = Some(outcome);
                Ok(())
            }
        }
    }
    fn poll(&mut self, now: Instant) -> Option<Outcome> {
        if let Some(o) = self.done.take() {
            return Some(o);
        }
        let run = self.run.as_mut()?;
        // The request frame is queued until the owned child exists; moving it is this call's job.
        let flushed = run.worker.flush();
        let outcome = match flushed.and_then(|()| run.worker.inbound(2)) {
            Ok(frames) if !frames.is_empty() => {
                let parsed = (frames.len() == 1)
                    .then(|| serde_json::from_slice::<Value>(&frames[0]).ok())
                    .flatten()
                    .and_then(|v| Self::result_text(&v));
                Some(parsed.map_or(Outcome::Indeterminate, Outcome::Completed))
            }
            Ok(_) => {
                if run.worker.has_exited() {
                    // It ended without saying how: the command may have run.
                    Some(Outcome::Indeterminate)
                } else if now >= run.deadline {
                    Some(Outcome::Indeterminate)
                } else {
                    None
                }
            }
            Err(_) => Some(Outcome::Indeterminate),
        };
        if outcome.is_some() && !run.stopping {
            run.stopping = true;
            run.worker.begin_stop(now);
            self.pending_stop = true;
        }
        let _ = run.started;
        outcome
    }
    fn busy(&self) -> bool {
        self.run.is_some() || self.done.is_some()
    }
    fn begin_stop(&mut self, now: Instant) {
        if let Some(run) = &mut self.run {
            run.stopping = true;
            run.worker.begin_stop(now);
        }
        self.pending_stop = true;
        self.done = None;
    }
    fn poll_stop(&mut self, now: Instant) -> StopState {
        let Some(run) = &mut self.run else {
            self.pending_stop = false;
            return StopState::Reaped;
        };
        if !run.stopping {
            return StopState::Pending;
        }
        match run.worker.poll_stop(now) {
            StopState::Reaped => {
                self.run = None;
                self.pending_stop = false;
                StopState::Reaped
            }
            other => other,
        }
    }
}

#[cfg(test)]
mod tests;
