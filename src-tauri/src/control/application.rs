//! Explicit local application selection. The installed binary selects protocol 1. Protocol 2
//! requires a caller-owned, already prepared V8 connection; this module opens no paths, migrates
//! no databases and creates no listener. Both modes run one authority writer, never both.
use super::F8Transport;
use crate::{
    contracts::{
        WireUuid,
        control::{ControlError as E, ControlResult},
    },
    f8::{
        allocator::Allocator,
        authority::Authority,
        engine::{Engine, Spawner},
        registry::Registry,
        service::{SUPERVISOR_INTERVAL, Service},
        staging::StagingRoot,
        v8,
    },
};
use rusqlite::Connection;
use std::{
    collections::BTreeSet,
    os::unix::fs::MetadataExt,
    os::unix::net::UnixStream,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    time::{Duration, Instant},
};

// SQLite exclusive file locking excludes other processes/private caches, but shared-cache
// connections in this process can still write. Retain a process-local inode reservation too.
type FileIdentity = (u64, u64);
static WRITERS: OnceLock<Mutex<BTreeSet<FileIdentity>>> = OnceLock::new();
struct WriterGuard(Option<FileIdentity>);
impl WriterGuard {
    fn acquire(connection: &Connection) -> ControlResult<Self> {
        let Some(path) = connection.path().filter(|path| !path.is_empty()) else {
            // Production durable authorities require a file. Private in-memory images are
            // accepted only by unit fixtures, never a deployed/shared-memory authority.
            return if cfg!(test) {
                Ok(Self(None))
            } else {
                Err(E::UnsafePath)
            };
        };
        let metadata = std::fs::metadata(path).map_err(|_| E::UnsafePath)?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(E::UnsafePath);
        }
        let identity = (metadata.dev(), metadata.ino());
        if !WRITERS
            .get_or_init(Mutex::default)
            .lock()
            .map_err(|_| E::StorageUnavailable)?
            .insert(identity)
        {
            return Err(E::StorageUnavailable);
        }
        Ok(Self(Some(identity)))
    }
}
impl Drop for WriterGuard {
    fn drop(&mut self) {
        if let Some(identity) = self.0
            && let Some(writers) = WRITERS.get()
            && let Ok(mut writers) = writers.lock()
        {
            writers.remove(&identity);
        }
    }
}

pub enum LocalSelection<S: Spawner> {
    InstalledProtocol1,
    PreparedProtocol2(Box<PreparedApplication<S>>),
}

/// No fallback between versions. Supplying protocol-2 resources is a local Rust capability,
/// not an environment variable, Tauri command, client request or installed migration switch.
pub fn run_selected<S: Spawner>(selection: LocalSelection<S>) -> ControlResult<()> {
    match selection {
        LocalSelection::InstalledProtocol1 => super::server::run_from_environment(),
        LocalSelection::PreparedProtocol2(application) => application.run(),
    }
}

#[derive(Clone)]
pub struct ApplicationHandle {
    incoming: SyncSender<UnixStream>,
    stop: Arc<AtomicBool>,
}
impl ApplicationHandle {
    pub fn attach(&self, stream: UnixStream) -> ControlResult<()> {
        if self.stop.load(Ordering::Acquire) {
            return Err(E::StorageUnavailable);
        }
        self.incoming.try_send(stream).map_err(|error| match error {
            mpsc::TrySendError::Full(_) => E::CapacityExceeded,
            mpsc::TrySendError::Disconnected(_) => E::StorageUnavailable,
        })
    }
    /// Reserved shutdown capacity: a full client admission queue cannot delay this signal.
    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Release);
    }
}

pub struct PreparedApplication<S: Spawner> {
    transport: F8Transport<S>,
    incoming: Receiver<UnixStream>,
    stop: Arc<AtomicBool>,
    // Declared last: release the reservation after the transport/SQLite owner is dropped.
    _writer: WriterGuard,
}
impl<S: Spawner> PreparedApplication<S> {
    /// Validate the selected V8/V9/V10 image and recover predecessor work before accepting clients.
    /// Durable history seeds anti-reuse fencing; it never restores a live worker or lease.
    pub fn new(
        mut connection: Connection,
        authority: WireUuid,
        staging: Option<StagingRoot>,
        spawner: S,
    ) -> ControlResult<(Self, ApplicationHandle)> {
        if !connection.is_autocommit() {
            return Err(E::StorageIncompatible);
        }
        let writer = WriterGuard::acquire(&connection)?;
        connection
            .busy_timeout(Duration::ZERO)
            .map_err(|_| E::StorageUnavailable)?;
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|_| E::StorageUnavailable)?;
        if !matches!(version, 8..=10) {
            return Err(E::StorageIncompatible);
        }
        // A local prepared connection is not itself proof of exclusive authority. Hold
        // SQLite's exclusive connection lock across commits before inspecting/recovering
        // the image. This prevents a second prepared application from competing with it,
        // in both rollback and WAL modes, without creating another filesystem lock path.
        connection
            .pragma_update(None, "locking_mode", "EXCLUSIVE")
            .map_err(|_| E::StorageUnavailable)?;
        connection
            .execute_batch("BEGIN EXCLUSIVE; COMMIT;")
            .map_err(|_| E::StorageUnavailable)?;
        match version {
            10 => {
                crate::f8::v10::validate_v10(&connection)?;
            }
            9 => {
                crate::f8::v9::validate_v9(&connection)?;
            }
            _ => {
                v8::validate_v8(&connection)?;
            }
        }
        let (used, generations) =
            Allocator::new(&mut connection, authority.clone())?.registry_history()?;
        let registry = Registry::with_history(authority.clone(), used, generations)?;
        let engine = Engine::new(connection, authority, registry, staging)?;
        let authority = if version == 10 {
            #[cfg(target_os = "linux")]
            {
                Authority::new_guarded(engine)?
            }
            #[cfg(not(target_os = "linux"))]
            {
                return Err(E::Unsupported);
            }
        } else {
            Authority::new(engine)
        };
        let mut service = Service::new(authority, spawner);
        service.tick(Instant::now())?;
        Ok(Self::wrap(F8Transport::new(service), writer))
    }
    fn wrap(transport: F8Transport<S>, writer: WriterGuard) -> (Self, ApplicationHandle) {
        let (sender, incoming) = mpsc::sync_channel(super::server::MAX_CLIENTS);
        let stop = Arc::new(AtomicBool::new(false));
        let handle = ApplicationHandle {
            incoming: sender,
            stop: stop.clone(),
        };
        (
            Self {
                transport,
                incoming,
                stop,
                _writer: writer,
            },
            handle,
        )
    }
    #[cfg(test)]
    pub(crate) fn with_test_transport(transport: F8Transport<S>) -> (Self, ApplicationHandle) {
        Self::wrap(transport, WriterGuard(None))
    }
    pub fn service(&self) -> &Service<S> {
        self.transport.service()
    }
    pub fn transport(&self) -> &F8Transport<S> {
        &self.transport
    }
    pub fn transport_mut(&mut self) -> &mut F8Transport<S> {
        &mut self.transport
    }
    pub fn poll(&mut self, now: Instant) -> ControlResult<()> {
        if self.stop.load(Ordering::Acquire) {
            self.transport
                .service_mut()
                .authority_mut()
                .engine_mut()
                .shutdown();
            return self.transport.service_mut().tick(now);
        }
        // Bounded client admissions cannot starve runtime supervision or existing clients.
        for _ in 0..super::server::MAX_CLIENTS {
            match self.incoming.try_recv() {
                Ok(stream) => {
                    let _ = self.transport.attach(stream, now);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.stop.store(true, Ordering::Release);
                    break;
                }
            }
        }
        let result = self.transport.poll(now);
        if result.is_err() {
            self.stop.store(true, Ordering::Release);
            self.transport
                .service_mut()
                .authority_mut()
                .engine_mut()
                .shutdown();
        }
        result
    }
    pub fn run(mut self) -> ControlResult<()> {
        let mut failure = None;
        let mut shutdown_at = None;
        let mut served = self.transport.served();
        let mut busy_until = Instant::now();
        loop {
            let now = Instant::now();
            if let Err(error) = self.poll(now) {
                failure.get_or_insert(error);
            }
            if self.stop.load(Ordering::Acquire) {
                let began = *shutdown_at.get_or_insert(now);
                if self
                    .transport
                    .service()
                    .authority()
                    .engine()
                    .cleanup_complete()
                {
                    return failure.map_or(Ok(()), Err);
                }
                if now.saturating_duration_since(began) >= Duration::from_secs(5) {
                    // Drop retains child ownership through the bounded reaper. An incomplete
                    // shutdown is never reported as a successful durable exit.
                    return Err(failure.unwrap_or(E::Timeout));
                }
            }
            // A client that is talking is answered within a few milliseconds; once it falls
            // silent the loop is back at its supervision interval, and nothing is polled faster
            // than that while nobody asks.
            if self.transport.served() != served {
                served = self.transport.served();
                busy_until = now + BUSY_WINDOW;
            }
            let interval = if now < busy_until {
                BUSY_INTERVAL
            } else {
                SUPERVISOR_INTERVAL
            };
            std::thread::sleep(interval.saturating_sub(now.elapsed()));
        }
    }
}

/// While a client is talking, and for this long after, the loop polls at `BUSY_INTERVAL`.
const BUSY_WINDOW: Duration = Duration::from_millis(300);
const BUSY_INTERVAL: Duration = Duration::from_millis(4);

struct NeverSpawn;
impl Spawner for NeverSpawn {
    fn spawn(
        &mut self,
        _: &crate::f8::protocol::RuntimeRef,
        _: Option<&mut crate::f8::staging::LaunchStaging>,
    ) -> ControlResult<crate::f8::engine::Spawned> {
        Err(E::Unsupported)
    }
}
pub fn run_from_environment() -> ControlResult<()> {
    run_selected::<NeverSpawn>(LocalSelection::InstalledProtocol1)
}
