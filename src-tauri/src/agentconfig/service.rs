//! The inventory as a service: a current reading with a revision, a log of what changed
//! between revisions, and a watcher that rescans when a file of a provider changes
//! (F1-07, F1-08; AC-F1-02).
//!
//! The watcher only says *that* something may have changed. It never reads a file itself:
//! every rescan is the same bounded `inventory` call, through the same approved roots, so
//! a link planted in a watched directory teaches the watcher nothing. Changes are
//! debounced; a lost event (the queue overflowed) or a place that could not be watched is
//! made up for by a rescan at a long interval, so the view converges either way.
//!
//! Idle cost is nothing but the fallback tick: the thread sleeps in one `poll` on the
//! inotify descriptor and a wake pipe, with no polling interval of its own.
use super::inventory::{Environment, Inventory, SourceKey, inventory};
use super::model::Bounds;
use nix::{
    errno::Errno,
    poll::{PollFd, PollFlags, PollTimeout, poll},
    sys::inotify::{AddWatchFlags, InitFlags, Inotify},
    unistd::{pipe2, read, write},
};
use std::{
    collections::{BTreeSet, VecDeque},
    ffi::OsStr,
    os::fd::{AsFd, AsRawFd, OwnedFd},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// How long the watcher waits for a burst of events to end before it rescans.
const QUIET: Duration = Duration::from_millis(200);
/// The longest a burst may delay a rescan, so a file written all the time still shows.
const LONGEST_BURST: Duration = Duration::from_secs(1);
/// A rescan with no event at all, for what no watch can see (a place that did not exist,
/// a lost event, a filesystem that does not report).
const FALLBACK: Duration = Duration::from_secs(30);
/// How many revisions back a window can ask for; older asks get a full resync.
const LOG: usize = 64;

/// What the window gets: a revision and the whole inventory at it.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub revision: u64,
    pub inventory: Arc<Inventory>,
}

/// What changed since a revision the window has.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case", tag = "result")]
pub enum Changes {
    /// Nothing since that revision.
    Same { revision: u64 },
    /// These sources differ; the window asks for the inventory again.
    Changed {
        revision: u64,
        sources: Vec<SourceKey>,
    },
    /// The revision is not one this service can account for from (it is older than the
    /// log, or from before a restart): read the inventory whole.
    Resync { revision: u64 },
}

struct State {
    revision: u64,
    inventory: Arc<Inventory>,
    log: VecDeque<(u64, Vec<SourceKey>)>,
}

struct Inner {
    environment: Mutex<Environment>,
    bounds: Bounds,
    state: Mutex<State>,
    stop_signal: OwnedFd,
}

/// The service. Cloning shares it; the watcher stops when the last clone is dropped or
/// `shutdown` is called.
#[derive(Clone)]
pub struct InventoryService {
    inner: Arc<Inner>,
    thread: Arc<Mutex<Option<JoinHandle<()>>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic elsewhere must not make the inventory unreadable: the data is a value.
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl InventoryService {
    /// Reads once, then watches. `None` when the bounds or the environment cannot make
    /// an inventory at all (a programming error, not a state of the machine).
    pub fn start(environment: Environment, bounds: Bounds) -> Option<Self> {
        // The watches come first: a file edited between the first reading and the
        // watches being in place would otherwise wait for the fallback rescan.
        let inotify = Inotify::init(InitFlags::IN_CLOEXEC | InitFlags::IN_NONBLOCK).ok();
        let watches = inotify
            .as_ref()
            .map(|inotify| watch_directories(inotify, &watched_directories(&environment)))
            .unwrap_or_default();
        let first = inventory(&environment, bounds).ok()?;
        let (wake, stop_signal) =
            pipe2(nix::fcntl::OFlag::O_CLOEXEC | nix::fcntl::OFlag::O_NONBLOCK).ok()?;
        let inner = Arc::new(Inner {
            environment: Mutex::new(environment),
            bounds,
            state: Mutex::new(State {
                revision: 1,
                inventory: Arc::new(first),
                log: VecDeque::new(),
            }),
            stop_signal,
        });
        let watcher = Arc::clone(&inner);
        let thread = std::thread::Builder::new()
            .name("agentconfig-watch".into())
            .spawn(move || watch(&watcher, &wake, inotify, watches))
            .ok()?;
        Some(Self {
            inner,
            thread: Arc::new(Mutex::new(Some(thread))),
        })
    }

    pub fn snapshot(&self) -> Snapshot {
        let state = lock(&self.inner.state);
        Snapshot {
            revision: state.revision,
            inventory: Arc::clone(&state.inventory),
        }
    }

    pub fn changes_since(&self, since: u64) -> Changes {
        let state = lock(&self.inner.state);
        let revision = state.revision;
        if since == revision {
            return Changes::Same { revision };
        }
        let covered = since < revision
            && state
                .log
                .front()
                .is_some_and(|(oldest, _)| *oldest <= since + 1);
        if !covered {
            return Changes::Resync { revision };
        }
        let sources: BTreeSet<SourceKey> = state
            .log
            .iter()
            .filter(|(at, _)| *at > since)
            .flat_map(|(_, sources)| sources.iter().copied())
            .collect();
        Changes::Changed {
            revision,
            sources: sources.into_iter().collect(),
        }
    }

    /// Selects the project whose own configuration is read as well, or none. The path is
    /// one the backend got from the native folder dialog, never from the window's text.
    pub fn select_project(&self, project: Option<PathBuf>) {
        lock(&self.inner.environment).project = project;
        self.rescan();
    }

    /// Reads again now. Returns whether anything differed.
    pub fn rescan(&self) -> bool {
        self.inner.rescan()
    }

    /// Stops the watcher and waits for it.
    pub fn shutdown(&self) {
        self.inner.wake();
        if let Some(thread) = lock(&self.thread).take() {
            let _ = thread.join();
        }
    }
}

impl Drop for InventoryService {
    fn drop(&mut self) {
        // The last clone ends the watcher; the others leave it running.
        if Arc::strong_count(&self.inner) <= 2 {
            self.shutdown();
        }
    }
}

impl Inner {
    fn wake(&self) {
        // A full pipe already means the watcher has been told.
        let _ = write(&self.stop_signal, b"x");
    }

    fn rescan(&self) -> bool {
        let environment = lock(&self.environment).clone();
        let Ok(next) = inventory(&environment, self.bounds) else {
            return false;
        };
        let mut state = lock(&self.state);
        if *state.inventory == next {
            return false;
        }
        let sources = differing_sources(&state.inventory, &next);
        state.revision += 1;
        let revision = state.revision;
        state.inventory = Arc::new(next);
        state.log.push_back((revision, sources));
        while state.log.len() > LOG {
            state.log.pop_front();
        }
        true
    }

    fn directories(&self) -> Vec<PathBuf> {
        watched_directories(&lock(&self.environment))
    }
}

/// The sources whose report or items differ between two readings.
fn differing_sources(old: &Inventory, new: &Inventory) -> Vec<SourceKey> {
    let keys: BTreeSet<SourceKey> = old
        .sources
        .iter()
        .chain(&new.sources)
        .map(|source| source.key)
        .collect();
    keys.into_iter()
        .filter(|key| {
            let report = |inventory: &Inventory| {
                inventory
                    .sources
                    .iter()
                    .find(|source| source.key == *key)
                    .cloned()
            };
            let items = |inventory: &Inventory| {
                inventory
                    .items
                    .iter()
                    .filter(|item| item.source == *key)
                    .cloned()
                    .collect::<Vec<_>>()
            };
            report(old) != report(new) || items(old) != items(new)
        })
        .collect()
}

/// The directories whose events can mean a source changed: each provider's own, its
/// skills and rules folders, the home (for `~/.claude.json` and a provider directory
/// appearing) and the same for a selected project. System places are left to the
/// fallback rescan: `/etc` is far too busy to watch for two files.
fn watched_directories(environment: &Environment) -> Vec<PathBuf> {
    let (claude, codex) = (environment.claude_dir(), environment.codex_dir());
    let mut directories = vec![
        environment.home.clone(),
        claude.join("skills"),
        claude.join("rules"),
        claude,
        codex,
    ];
    if let Some(project) = &environment.project {
        directories.push(project.clone());
        directories.push(project.join(".claude"));
        directories.push(project.join(".claude/skills"));
        directories.push(project.join(".claude/rules"));
        directories.push(project.join(".codex"));
    }
    directories.sort();
    directories.dedup();
    directories
}

/// Whether an event in a directory can matter. The home and the project root are busy
/// places; only the names the adapters care about count there.
fn relevant(directory: &Path, name: Option<&OsStr>, environment: &Environment) -> bool {
    let Some(name) = name.and_then(OsStr::to_str) else {
        return true;
    };
    let busy = directory == environment.home || environment.project.as_deref() == Some(directory);
    !busy
        || matches!(
            name,
            ".claude.json" | ".claude" | ".codex" | ".mcp.json" | "CLAUDE.md" | "AGENTS.md"
        )
        || Some(OsStr::new(name))
            == environment
                .claude_config_dir
                .as_deref()
                .and_then(Path::file_name)
        || Some(OsStr::new(name)) == environment.codex_home.as_deref().and_then(Path::file_name)
}

fn watch_flags() -> AddWatchFlags {
    AddWatchFlags::IN_CLOSE_WRITE
        | AddWatchFlags::IN_MOVED_TO
        | AddWatchFlags::IN_MOVED_FROM
        | AddWatchFlags::IN_CREATE
        | AddWatchFlags::IN_DELETE
        | AddWatchFlags::IN_DELETE_SELF
        | AddWatchFlags::IN_MOVE_SELF
        | AddWatchFlags::IN_ATTRIB
        | AddWatchFlags::IN_ONLYDIR
        | AddWatchFlags::IN_DONT_FOLLOW
}

type Watch = (nix::sys::inotify::WatchDescriptor, PathBuf);

/// Puts a watch on every directory that exists. Adding one already there is harmless.
fn watch_directories(inotify: &Inotify, directories: &[PathBuf]) -> Vec<Watch> {
    directories
        .iter()
        .filter_map(|directory| {
            inotify
                .add_watch(directory, watch_flags())
                .ok()
                .map(|descriptor| (descriptor, directory.clone()))
        })
        .collect()
}

fn ready(fds: &mut [PollFd<'_>], timeout: Duration) -> bool {
    let millis = u16::try_from(timeout.as_millis()).unwrap_or(u16::MAX);
    loop {
        match poll(fds, PollTimeout::from(millis)) {
            Ok(count) => return count > 0,
            Err(Errno::EINTR) => (),
            Err(_) => return false,
        }
    }
}

/// The watcher's loop. Without inotify (`None`) the service still works: it rescans at
/// the fallback interval.
fn watch(inner: &Inner, wake: &OwnedFd, inotify: Option<Inotify>, mut watches: Vec<Watch>) {
    loop {
        let mut fds = vec![PollFd::new(wake.as_fd(), PollFlags::POLLIN)];
        if let Some(inotify) = &inotify {
            fds.push(PollFd::new(inotify.as_fd(), PollFlags::POLLIN));
        }
        let signalled = ready(&mut fds, FALLBACK);
        if fds[0]
            .revents()
            .is_some_and(|flags| flags.contains(PollFlags::POLLIN))
        {
            let mut sink = [0_u8; 8];
            let _ = read(wake.as_raw_fd(), &mut sink);
            return;
        }
        let mut changed = !signalled; // the fallback tick: look anyway
        if let (true, Some(inotify)) = (signalled, &inotify) {
            changed = drain(inotify, &watches, &lock(&inner.environment));
            // A burst: wait for it to settle, but not for ever.
            let started = Instant::now();
            while changed && started.elapsed() < LONGEST_BURST {
                let mut fds = [PollFd::new(inotify.as_fd(), PollFlags::POLLIN)];
                if !ready(&mut fds, QUIET) {
                    break;
                }
                drain(inotify, &watches, &lock(&inner.environment));
            }
        }
        if changed {
            inner.rescan();
            if let Some(inotify) = &inotify {
                // A directory may have appeared, or been replaced.
                watches = watch_directories(inotify, &inner.directories());
            }
        }
    }
}

/// Reads every queued event. True when one can mean a source changed, or when events
/// were lost.
fn drain(inotify: &Inotify, watches: &[Watch], environment: &Environment) -> bool {
    let mut relevant_event = false;
    loop {
        match inotify.read_events() {
            Ok(events) if events.is_empty() => return relevant_event,
            Ok(events) => {
                for event in events {
                    if event.mask.contains(AddWatchFlags::IN_Q_OVERFLOW) {
                        relevant_event = true;
                        continue;
                    }
                    let directory = watches
                        .iter()
                        .find(|(descriptor, _)| *descriptor == event.wd)
                        .map(|(_, path)| path.as_path());
                    // A watch that was dropped says so; the next rescan re-adds it.
                    relevant_event |= directory.is_none_or(|directory| {
                        relevant(directory, event.name.as_deref(), environment)
                    });
                }
            }
            Err(_) => return relevant_event,
        }
    }
}
