//! The protocol-2 authority as a process of its own.
//!
//! **Selected by the caller, never discovered.** `wyss-control` without arguments is the
//! installed protocol-1 authority and stays that. Protocol 2 runs only when the command line
//! says `--protocol 2` and names both directories it uses; nothing here reads an environment
//! variable, a configuration file or the protocol-1 store, and a failure never falls back to
//! the other version.
//!
//! **Its own state.** The durable image is `authority.sqlite3` in the named state directory,
//! a different file from the protocol-1 `control.sqlite3`. A directory that holds a
//! protocol-1 store is refused: the two versions never share a directory, so neither can
//! open, migrate or replace the other's data. An image is created only on request (`--create`,
//! or `--migrate-copy` for a rehearsal on a copy the caller made) and only where none exists;
//! an existing one must already be a complete protocol-2 image, which
//! `PreparedApplication::new` validates before a client is accepted.
//!
//! **Its own socket.** A private socket in the named runtime directory, bound under the
//! protocol-1 rules (a live listener is never unlinked) and served to peers of this user only.
//! The two directories carry the same exclusive locks as protocol 1, so a second authority on
//! the same directories, of either version, is refused.
use super::{
    application::{ApplicationHandle, LocalSelection, PreparedApplication, run_selected},
    paths::{Paths, validate_file},
    peer,
    server::bind,
    store::uuid,
};
use crate::{
    contracts::control::{ControlError as E, ControlResult},
    f8::{
        engine::Spawner,
        migration,
        production::{CodexConfig, CodexSpawner},
        staging::StagingRoot,
    },
};
use nix::{
    errno::Errno,
    poll::{PollFd, PollFlags, poll},
    sys::{
        signal::{SigSet, Signal},
        signalfd::{SfdFlags, SignalFd},
        stat::{Mode, umask},
    },
};
use rusqlite::Connection;
use std::{
    ffi::OsString,
    io::ErrorKind,
    os::{fd::AsFd, unix::net::UnixListener},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

/// The durable image's name in the state directory. Never `control.sqlite3`.
const IMAGE: &str = "authority.sqlite3";
/// The protocol-1 store's name: seeing it in the state directory refuses the start.
const PROTOCOL_1_STORE: &str = "control.sqlite3";

/// Where the authority's durable image comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Image {
    /// The image already in the state directory.
    Existing,
    /// A new image with no history. Refused when an image exists.
    Create,
    /// A new image migrated from a copy of a protocol-1 store that the caller made and owns.
    /// The copy is read, never changed. Refused when an image exists.
    MigrateCopy(PathBuf),
}

/// What a protocol-2 start was told, all of it explicit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Serve {
    pub runtime_root: PathBuf,
    pub state_root: PathBuf,
    pub image: Image,
    /// Codex, when the operator named it. Without it no runtime can be started.
    pub codex: Option<CodexConfig>,
}
impl Serve {
    /// Parses `--protocol 2 --runtime-root DIR --state-root DIR [--create | --migrate-copy FILE]
    /// [--codex PATH --codex-home DIR [--allow-credential]]` (without the program name). Every other shape is refused: an unknown flag, a repeated
    /// one, a relative path, another version.
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> ControlResult<Self> {
        let mut args = args.into_iter();
        let (mut protocol, mut runtime, mut state, mut image) = (false, None, None, None);
        let (mut codex, mut home, mut credential) = (None, None, false);
        while let Some(flag) = args.next() {
            match flag.to_str() {
                Some("--protocol") if !protocol => {
                    if take(&mut args)? != "2" {
                        return Err(E::InvalidRequest);
                    }
                    protocol = true;
                }
                Some("--runtime-root") if runtime.is_none() => {
                    runtime = Some(absolute(take(&mut args)?)?);
                }
                Some("--state-root") if state.is_none() => {
                    state = Some(absolute(take(&mut args)?)?);
                }
                Some("--codex") if codex.is_none() => codex = Some(absolute(take(&mut args)?)?),
                Some("--codex-home") if home.is_none() => home = Some(absolute(take(&mut args)?)?),
                Some("--allow-credential") if !credential => credential = true,
                Some("--create") if image.is_none() => image = Some(Image::Create),
                Some("--migrate-copy") if image.is_none() => {
                    image = Some(Image::MigrateCopy(absolute(take(&mut args)?)?));
                }
                _ => return Err(E::InvalidRequest),
            }
        }
        // Codex and its home go together; a credential is allowed only where there is a Codex.
        let codex = match (codex, home, credential) {
            (None, None, false) => None,
            (Some(executable), Some(home), allow_credential) => Some(CodexConfig {
                executable,
                home,
                allow_credential,
            }),
            _ => return Err(E::InvalidRequest),
        };
        match (protocol, runtime, state) {
            (true, Some(runtime_root), Some(state_root)) => Ok(Self {
                runtime_root,
                state_root,
                image: image.unwrap_or(Image::Existing),
                codex,
            }),
            _ => Err(E::InvalidRequest),
        }
    }
}
fn take(args: &mut impl Iterator<Item = OsString>) -> ControlResult<OsString> {
    args.next().ok_or(E::InvalidRequest)
}
fn absolute(value: OsString) -> ControlResult<PathBuf> {
    let path = PathBuf::from(value);
    if path.is_absolute() && path.as_os_str().len() < 100 {
        Ok(path)
    } else {
        Err(E::UnsafePath)
    }
}

/// The spawner of an authority that is not allowed to start a runtime. A session can be
/// prepared, a lease granted and the history read; a runtime start is refused, always.
pub struct NoRuntime;
impl Spawner for NoRuntime {
    fn spawn(
        &mut self,
        _: &crate::f8::protocol::RuntimeRef,
        _: Option<&mut crate::f8::staging::LaunchStaging>,
    ) -> ControlResult<crate::f8::engine::Spawned> {
        Err(E::Unsupported)
    }
}

fn exists(path: &Path) -> ControlResult<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(_) => Err(E::UnsafePath),
    }
}

/// Opens (or creates) the image under the rules of `Serve`, and returns it ready for
/// `PreparedApplication::new`.
fn open_image(state: &Path, image: &Image) -> ControlResult<Connection> {
    if exists(&state.join(PROTOCOL_1_STORE))? {
        // A protocol-1 store is there: whatever this directory is for, it is not this.
        return Err(E::StorageIncompatible);
    }
    let path = state.join(IMAGE);
    match (image, exists(&path)?) {
        (Image::Existing, true) => validate_file(&path)?,
        (Image::Existing, false) => return Err(E::StorageUnavailable),
        (_, true) => return Err(E::UnsafePath),
        (Image::Create, false) => migration::fresh()?.write_new_image(&path)?,
        (Image::MigrateCopy(copy), false) => {
            if copy.file_name().is_some_and(|n| n == PROTOCOL_1_STORE) {
                // The installed store has this name; a copy is called something else.
                return Err(E::UnsafePath);
            }
            // A file that exists, is the caller's, has one name and is not the installed store
            // under another name; opened without the right to create it.
            validate_file(copy)?;
            let canonical = std::fs::canonicalize(copy).map_err(|_| E::UnsafePath)?;
            if canonical.file_name().is_some_and(|n| n == PROTOCOL_1_STORE) {
                return Err(E::UnsafePath);
            }
            let mut source = Connection::open_with_flags(
                &canonical,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                    | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .map_err(|_| E::StorageUnavailable)?;
            migration::prepare(&mut source)?.write_new_image(&path)?;
        }
    }
    Connection::open(&path).map_err(|_| E::StorageUnavailable)
}

/// Accepts clients until a stop signal or until the application ends. The listener is
/// nonblocking; each peer is checked as protocol 1 checks it before it is handed over.
fn accept(
    listener: UnixListener,
    handle: ApplicationHandle,
    mut signals: SignalFd,
    done: Arc<AtomicBool>,
) {
    while !done.load(Ordering::Acquire) {
        let mut fds = [
            PollFd::new(listener.as_fd(), PollFlags::POLLIN),
            PollFd::new(signals.as_fd(), PollFlags::POLLIN),
        ];
        match poll(&mut fds, 250u16) {
            Ok(_) => (),
            Err(Errno::EINTR) => continue,
            Err(_) => {
                handle.shutdown();
                return;
            }
        }
        if matches!(signals.read_signal(), Ok(Some(_))) {
            handle.shutdown();
            return;
        }
        // Bounded per wake, so a flood cannot starve the stop signal.
        for _ in 0..super::server::MAX_CLIENTS {
            match listener.accept() {
                Ok((stream, _)) => {
                    if peer(&stream).is_ok() {
                        let _ = handle.attach(stream);
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(_) => {
                    handle.shutdown();
                    return;
                }
            }
        }
    }
}

/// Runs the authority until it is asked to stop. `spawner` is made once the directories are
/// safe, from the private product directory the image lives in.
pub fn serve<S: Spawner>(
    config: &Serve,
    precheck: impl FnOnce() -> ControlResult<()>,
    spawner: impl FnOnce(&Path, &Path) -> ControlResult<(S, Option<StagingRoot>)>,
) -> ControlResult<()> {
    let mut mask = SigSet::empty();
    mask.add(Signal::SIGTERM);
    mask.add(Signal::SIGINT);
    mask.thread_block().map_err(|_| E::InternalError)?;
    let signals = SignalFd::with_flags(&mask, SfdFlags::SFD_NONBLOCK | SfdFlags::SFD_CLOEXEC)
        .map_err(|_| E::InternalError)?;
    // Everything this process creates is private to its user.
    umask(Mode::from_bits_truncate(0o077));
    let paths = Paths::new(&config.runtime_root, &config.state_root)?;
    let _locks = paths.lock()?;
    // What can refuse the start without creating anything comes first, and what creates
    // something only once the image is: a refused start leaves no image or directory behind.
    precheck()?;
    let connection = open_image(&paths.state, &config.image)?;
    let (spawner, staging) = spawner(&paths.runtime, &paths.state)?;
    let (application, handle) = PreparedApplication::new(connection, uuid(), staging, spawner)?;
    let (listener, _socket) = bind(&paths.runtime.join("control.sock"))?;
    let done = Arc::new(AtomicBool::new(false));
    let acceptor = {
        let (handle, done) = (handle.clone(), done.clone());
        thread::spawn(move || accept(listener, handle, signals, done))
    };
    let result = run_selected(LocalSelection::PreparedProtocol2(Box::new(application)));
    done.store(true, Ordering::Release);
    let _ = acceptor.join();
    result
}

/// What `wyss-control` does when it was given arguments: a protocol-2 authority. With
/// `--codex` it starts Codex in the isolated local-tools namespace; without it, no runtime.
pub fn run_from_arguments(args: impl IntoIterator<Item = OsString>) -> ControlResult<()> {
    let config = Serve::parse(args)?;
    match config.codex.clone() {
        Some(codex) => serve(
            &config,
            || codex.check().map(|_| ()),
            |runtime, state| {
                let (spawner, staging) = CodexSpawner::new(codex.clone(), runtime, state)?;
                Ok((spawner, Some(staging)))
            },
        ),
        None => serve(&config, || Ok(()), |_, _| Ok((NoRuntime, None))),
    }
}

#[cfg(test)]
mod tests;
