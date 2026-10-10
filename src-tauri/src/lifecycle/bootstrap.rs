//! The one bootstrap of a desktop process.
//!
//! It follows the authority link and acts in exactly two cases: the first time no
//! authority answers, and when the user asks for a retry. Acting means checking what is
//! installed, writing what is missing, asking the service manager to start the unit, and
//! then waiting, for a bounded time, for the link to connect. It never decides that the
//! authority is ready: the link does, by connecting.
use super::{
    Layout, LifecycleError, LifecycleResult,
    install::{self, FileState},
    manager::{ActiveState, LoadState, ManagerError, ServiceManager, UnitResult, UnitStatus},
    package::Provision,
    view::{BuildKind, Failure, InstallationState, LifecycleView, Origin, Phase, ServiceState},
};
use crate::authority::{AuthorityService, Connection, Fault};
use nix::{
    sys::socket::{getsockopt, sockopt::PeerCredentials},
    unistd::geteuid,
};
use std::{
    os::unix::{
        fs::{FileTypeExt, MetadataExt},
        net::UnixStream,
    },
    sync::{
        Arc, Condvar, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

/// The bounds of one attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// How long an attempt waits for the link to connect once the authority is started.
    pub startup: Duration,
    /// The first look at the unit while waiting. Each later one waits twice as long, up
    /// to four seconds.
    pub first_check: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            startup: Duration::from_secs(20),
            first_check: Duration::from_millis(500),
        }
    }
}
const CHECK_LIMIT: Duration = Duration::from_secs(4);
/// Longest the follower blocks on a link that does not change.
const WATCH: Duration = Duration::from_secs(20);
const JOIN_LIMIT: Duration = Duration::from_secs(3);

/// What the link says, reduced to what the lifecycle acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    /// The first attempt has not ended yet.
    Connecting,
    Connected,
    /// Nothing answers: there is no socket, or what was connected is gone.
    Absent,
    /// Something answers that this build cannot work with.
    Incompatible,
    /// The place of the socket is not safely this user's.
    Unsafe,
    /// The desktop is closing.
    Stopped,
}
/// The observer of the connection. `AuthorityService` is the one in the product.
pub trait AuthorityLink: Send + Sync + 'static {
    /// The link's revision and state: at once for `None`, otherwise once it differs from
    /// revision `after` or `timeout` passed.
    fn observe(&self, after: Option<u64>, timeout: Duration) -> (u64, LinkState);
}
impl AuthorityLink for AuthorityService {
    fn observe(&self, after: Option<u64>, timeout: Duration) -> (u64, LinkState) {
        let view = self.wait_changed(after, timeout);
        let state = match (view.connection, view.reason) {
            (Connection::Connected, _) => LinkState::Connected,
            (Connection::Stopped, _) => LinkState::Stopped,
            // The reason of the last attempt stands while the next one is made.
            (_, Some(Fault::Absent | Fault::Lost)) => LinkState::Absent,
            (_, Some(Fault::Incompatible | Fault::Malformed)) => LinkState::Incompatible,
            (_, Some(Fault::UnsafePath)) => LinkState::Unsafe,
            (_, None) => LinkState::Connecting,
        };
        (view.revision, state)
    }
}

/// Why an attempt is made, which is what it may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// The first time nothing answered. May install and start.
    Automatic,
    /// The user asked. May install and start, and may clear a recorded failure first.
    Retry,
    /// An authority that was connected went away. Only watches: whether it comes back is
    /// the service manager's business.
    Recovery,
}

struct Inner {
    view: Mutex<LifecycleView>,
    changed: Condvar,
    /// One attempt at a time, whoever asks.
    attempting: AtomicBool,
    /// The one automatic start of this process was used.
    started_once: AtomicBool,
    stop: AtomicBool,
    link: Arc<dyn AuthorityLink>,
    provision: Provision,
    layout: LifecycleResult<Layout>,
    manager: Arc<dyn ServiceManager>,
    limits: Limits,
}

/// Ends an attempt even if it panics, so that Retry is never left unusable.
struct Attempt<'a>(&'a Inner);
impl Drop for Attempt<'_> {
    fn drop(&mut self) {
        self.0.attempting.store(false, Ordering::Release);
    }
}

fn failure_of(error: LifecycleError) -> Failure {
    match error {
        LifecycleError::UnsafePath => Failure::InstallationUnsafe,
        LifecycleError::Foreign => Failure::InstallationMismatch,
        LifecycleError::PackageInvalid => Failure::PackageInvalid,
        LifecycleError::Io | LifecycleError::Busy => Failure::InstallFailed,
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, LifecycleView> {
        // Every update replaces whole fields in one step: coherent after a panic.
        self.view.lock().unwrap_or_else(PoisonError::into_inner)
    }
    fn update(&self, change: impl FnOnce(&mut LifecycleView)) {
        let mut view = self.lock();
        let mut draft = view.clone();
        change(&mut draft);
        draft.failure = draft.failure.filter(|_| draft.phase == Phase::Failed);
        draft.retry = draft.phase == Phase::Failed;
        if !matches!(draft.phase, Phase::Ready | Phase::Incompatible) {
            draft.origin = None;
        }
        draft.update_pending = draft.phase == Phase::Ready
            && draft.origin == Some(Origin::Managed)
            && draft.diagnostics.installation == InstallationState::Outdated;
        if draft != *view {
            draft.revision = view.revision + 1;
            *view = draft;
            self.changed.notify_all();
        }
    }
    fn phase(&self, phase: Phase) {
        self.update(|view| view.phase = phase);
    }
    fn stopping(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }

    /// The process the listener of the control socket belongs to, as the kernel recorded
    /// it when the socket was created. A connection is opened and closed; nothing is sent.
    fn listener(&self) -> Option<u32> {
        let socket = self.layout.as_ref().ok()?.socket();
        let meta = std::fs::symlink_metadata(&socket).ok()?;
        if !meta.file_type().is_socket() || meta.uid() != geteuid().as_raw() {
            return None;
        }
        let stream = UnixStream::connect(&socket).ok()?;
        let peer = getsockopt(&stream, PeerCredentials).ok()?;
        (peer.uid() == geteuid().as_raw())
            .then(|| u32::try_from(peer.pid()).ok())
            .flatten()
    }
    /// Whether the authority that answers is this installation's service: the manager
    /// runs the unit from this installation's file, and the process it names is the one
    /// that listens. Both are read now, about a process that is alive to be connected to.
    fn origin(&self, status: Option<&UnitStatus>) -> Option<Origin> {
        let listener = self.listener()?;
        let unit = self.layout.as_ref().ok().map(Layout::unit);
        let managed = status.is_some_and(|status| {
            status.active == ActiveState::Active
                && status.main_pid == Some(listener)
                && status.fragment.is_some()
                && status.fragment == unit
        });
        Some(if managed {
            Origin::Managed
        } else {
            Origin::External
        })
    }
    /// What is installed and what the manager says, for the diagnostics. Read when an
    /// attempt ends and when the link settles: never on a timer.
    fn examine(&self) -> (InstallationState, ServiceState, Option<UnitStatus>) {
        let digest = match &self.provision {
            Provision::Packaged(package) => Some(package.digest()),
            _ => None,
        };
        let installation = match &self.layout {
            Err(_) => InstallationState::Unsafe,
            Ok(layout) => {
                let installed = install::inspect(layout, digest);
                match (installed.binary, installed.unit) {
                    (FileState::Unsafe, _) | (_, FileState::Unsafe) => InstallationState::Unsafe,
                    (_, FileState::Foreign) | (FileState::Foreign, _) => {
                        InstallationState::Mismatch
                    }
                    (FileState::Absent, _) | (_, FileState::Absent) => InstallationState::Absent,
                    (FileState::Current, FileState::Current) => InstallationState::Current,
                    _ => InstallationState::Outdated,
                }
            }
        };
        let status = self.manager.status();
        let service = match &status {
            Err(_) => ServiceState::Unavailable,
            Ok(status) => match (status.load, status.active) {
                (LoadState::NotFound, _) => ServiceState::NotInstalled,
                (_, ActiveState::Active) => ServiceState::Running,
                (_, ActiveState::Activating) => ServiceState::Starting,
                (_, ActiveState::Failed) => ServiceState::Failed,
                (_, ActiveState::Inactive | ActiveState::Deactivating) => ServiceState::Inactive,
                (_, ActiveState::Other) => ServiceState::Unknown,
            },
        };
        (installation, service, status.ok())
    }

    /// Follows what the link says while no attempt is being made.
    fn settle(&self, state: LinkState) {
        match state {
            LinkState::Connected | LinkState::Incompatible => {
                let (installation, service, status) = self.examine();
                let origin = self.origin(status.as_ref());
                self.update(|view| {
                    view.phase = if state == LinkState::Connected {
                        Phase::Ready
                    } else {
                        Phase::Incompatible
                    };
                    view.origin = origin;
                    view.diagnostics.installation = installation;
                    view.diagnostics.service = service;
                });
            }
            LinkState::Unsafe => self.fail(Failure::SocketUnsafe),
            LinkState::Connecting => self.phase(Phase::Checking),
            LinkState::Absent | LinkState::Stopped => (),
        }
    }
    fn fail(&self, failure: Failure) {
        let (installation, service, _) = self.examine();
        self.update(|view| {
            view.phase = Phase::Failed;
            view.failure = Some(failure);
            view.diagnostics.installation = installation;
            view.diagnostics.service = service;
        });
    }

    /// One bounded attempt. The caller holds `attempting`.
    fn attempt(&self, kind: Kind) {
        let _attempt = Attempt(self);
        self.phase(Phase::Checking);
        match self.carry_out(kind) {
            Ok(state) => self.settle(state),
            Err(failure) => self.fail(failure),
        }
    }
    fn carry_out(&self, kind: Kind) -> Result<LinkState, Failure> {
        // An authority that answers is used as it is: nothing is installed or started
        // for it, whoever started it.
        let (revision, state) = self.link.observe(None, Duration::ZERO);
        if matches!(state, LinkState::Connected | LinkState::Incompatible) {
            return Ok(state);
        }
        let status = self.manager.status();
        // A unit the manager runs, or is about to restart, is left to it. Its files are
        // not touched under it and it is not started a second time.
        let in_hand = status.as_ref().is_ok_and(UnitStatus::in_hand);
        if !in_hand {
            if kind == Kind::Recovery {
                return Err(match status {
                    Ok(status) if status.result == UnitResult::StartLimitHit => {
                        Failure::ExitedRepeatedly
                    }
                    _ => Failure::Stopped,
                });
            }
            let package = match &self.provision {
                Provision::Packaged(package) => package,
                Provision::Development => return Err(Failure::NotPackaged),
                Provision::Invalid => return Err(Failure::PackageInvalid),
            };
            let layout = self.layout.as_ref().map_err(|e| failure_of(*e))?;
            // Nothing is written for a manager that is not there to run it.
            let status = status.map_err(|_| Failure::ManagerUnavailable)?;
            let limited = status.result == UnitResult::StartLimitHit;
            // The manager stopped restarting it. Starting it again is the user's call.
            if limited && kind == Kind::Automatic {
                return Err(Failure::ExitedRepeatedly);
            }
            self.phase(Phase::Installing);
            let bytes = package.verified().map_err(failure_of)?;
            install::ensure(layout, &bytes, package.digest()).map_err(failure_of)?;
            drop(bytes);
            let refused = |error| match error {
                ManagerError::Unavailable => Failure::ManagerUnavailable,
                ManagerError::Refused => Failure::StartRefused,
            };
            // Cheap, and the only way the manager learns of a file that was just written.
            self.manager.reload().map_err(refused)?;
            if status.active == ActiveState::Failed {
                self.manager.reset_failed().map_err(refused)?;
            }
            self.phase(Phase::Starting);
            self.manager.start().map_err(refused)?;
        }
        self.phase(Phase::Connecting);
        self.await_connection(revision)
    }
    /// Waits for the link, looking at the unit a bounded number of times so that a start
    /// that failed is said when it is known and not when the limit passes.
    fn await_connection(&self, mut revision: u64) -> Result<LinkState, Failure> {
        let deadline = Instant::now() + self.limits.startup;
        let mut pause = self.limits.first_check;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let (next, state) = self.link.observe(Some(revision), pause.min(left));
            revision = next;
            match state {
                LinkState::Connected | LinkState::Incompatible | LinkState::Stopped => {
                    return Ok(state);
                }
                LinkState::Unsafe => return Err(Failure::SocketUnsafe),
                LinkState::Connecting | LinkState::Absent => (),
            }
            if self.stopping() {
                return Ok(LinkState::Stopped);
            }
            if Instant::now() >= deadline {
                return Err(Failure::SocketUnavailable);
            }
            match self.manager.status() {
                Ok(status) if status.in_hand() => (),
                Ok(status) if status.result == UnitResult::StartLimitHit => {
                    return Err(Failure::ExitedRepeatedly);
                }
                Ok(_) => return Err(Failure::StartupFailed),
                Err(_) => return Err(Failure::ManagerUnavailable),
            }
            pause = (pause * 2).min(CHECK_LIMIT);
        }
    }

    fn begin(&self) -> bool {
        self.attempting
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
    fn follow(self: &Arc<Self>) {
        let mut revision = None;
        // Whether the next absence is one to act on: the first, or one after a connection.
        let mut armed = true;
        loop {
            let (next, state) = self.link.observe(revision, WATCH);
            revision = Some(next);
            if self.stopping() || state == LinkState::Stopped {
                return;
            }
            let attempting = self.attempting.load(Ordering::Acquire);
            match state {
                LinkState::Connected => {
                    armed = true;
                    if !attempting {
                        self.settle(state);
                    }
                }
                LinkState::Absent if armed && self.begin() => {
                    armed = false;
                    let kind = if self.started_once.swap(true, Ordering::AcqRel) {
                        Kind::Recovery
                    } else {
                        Kind::Automatic
                    };
                    self.spawn(kind);
                }
                LinkState::Absent => (),
                _ if !attempting => self.settle(state),
                _ => (),
            }
        }
    }
    fn spawn(self: &Arc<Self>, kind: Kind) {
        let inner = self.clone();
        let spawned = thread::Builder::new()
            .name("wyss-authority-attempt".into())
            .spawn(move || inner.attempt(kind));
        if spawned.is_err() {
            self.attempting.store(false, Ordering::Release);
            self.fail(Failure::StartRefused);
        }
    }
}

/// The lifecycle of the authority as one desktop process sees it. Clones share one
/// state: every view of the application reads the same phase and none starts an attempt
/// of its own.
#[derive(Clone)]
pub struct AuthorityBootstrap {
    inner: Arc<Inner>,
    follower: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
}
impl AuthorityBootstrap {
    /// Starts following `link`. Nothing is installed or started by constructing this:
    /// that happens only if the link then reports that no authority answers.
    pub fn start(
        link: Arc<dyn AuthorityLink>,
        provision: Provision,
        layout: LifecycleResult<Layout>,
        manager: Arc<dyn ServiceManager>,
    ) -> Self {
        Self::start_with(link, provision, layout, manager, Limits::default())
    }
    /// `start` with other bounds than the product's.
    pub fn start_with(
        link: Arc<dyn AuthorityLink>,
        provision: Provision,
        layout: LifecycleResult<Layout>,
        manager: Arc<dyn ServiceManager>,
        limits: Limits,
    ) -> Self {
        let build = match &provision {
            Provision::Packaged(_) => BuildKind::Packaged,
            Provision::Development => BuildKind::Development,
            Provision::Invalid => BuildKind::Invalid,
        };
        let inner = Arc::new(Inner {
            view: Mutex::new(LifecycleView::initial(build)),
            changed: Condvar::new(),
            attempting: AtomicBool::new(false),
            started_once: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            link,
            provision,
            layout,
            manager,
            limits,
        });
        let follower = {
            let inner = inner.clone();
            thread::Builder::new()
                .name("wyss-authority-lifecycle".into())
                .spawn(move || inner.follow())
                .ok()
        };
        if follower.is_none() {
            inner.fail(Failure::StartRefused);
        }
        Self {
            inner,
            follower: Arc::new(Mutex::new(follower)),
        }
    }
    pub fn view(&self) -> LifecycleView {
        self.inner.lock().clone()
    }
    /// The current view, or the next one once it differs from revision `after`.
    pub fn wait_changed(&self, after: Option<u64>, timeout: Duration) -> LifecycleView {
        let deadline = Instant::now() + timeout;
        let mut view = self.inner.lock();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if after != Some(view.revision) || left.is_zero() || self.inner.stopping() {
                return view.clone();
            }
            view = self
                .inner
                .changed
                .wait_timeout(view, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
    /// The user's Retry. It starts one attempt if the last one failed and none is being
    /// made; otherwise it changes nothing. It answers at once: the attempt is followed
    /// through the view.
    pub fn retry(&self) -> LifecycleView {
        let failed = self.inner.lock().phase == Phase::Failed;
        if failed && !self.inner.stopping() && self.inner.begin() {
            // Shown at once, so that a second press finds nothing to retry.
            self.inner.phase(Phase::Checking);
            self.inner.spawn(Kind::Retry);
        }
        self.view()
    }
    /// Stops following. The authority is not told anything and is not stopped: its
    /// lifetime is the service manager's.
    pub fn shutdown(&self) {
        self.inner.stop.store(true, Ordering::Release);
        {
            let _view = self.inner.lock();
            self.inner.changed.notify_all();
        }
        let handle = self
            .follower
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(handle) = handle {
            // The link is stopped by its owner right before this; the follower sees it.
            let deadline = Instant::now() + JOIN_LIMIT;
            while !handle.is_finished() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
    }
}
