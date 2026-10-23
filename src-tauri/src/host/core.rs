//! What a host does (F9): owns the terminals and the native sessions, so that they outlive the
//! window. It is called by operation name with JSON parameters, from the socket server (the
//! `--host` process) or directly (the window's own in-process host, when none could be
//! reached). The terminal service underneath is the one a window always had; this adds a
//! reader per terminal that keeps a bounded scrollback, so a client can attach at any time.
use super::{
    protocol::HostError,
    ring::{self, Ring},
};
use crate::native::{
    profile::Body,
    service::{NativeError, NativeService, StartRequest},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};
use wyss_console::{
    contracts::identities::TerminalId,
    terminal::{
        TerminalService,
        contracts::{CreateTerminal, ErrorCode, Launch, TerminalError, TerminalInfo, TerminalSize},
    },
};

const MAX_READ: usize = 16 * 1024;
const MAX_WAIT: Duration = Duration::from_millis(2000);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Inner {
    ring: Ring,
    /// The terminal service refused a read: the terminal is closed.
    closed: bool,
    /// The process ended and everything it printed is in the ring.
    eof: bool,
    truncated: bool,
    status: TerminalInfo,
}

struct Entry {
    title: String,
    created_at: u64,
    inner: Mutex<Inner>,
    changed: Condvar,
}

pub struct HostCore {
    terminals: TerminalService,
    native: Option<NativeService>,
    table: Mutex<HashMap<String, Arc<Entry>>>,
    idle_since: Mutex<Option<Instant>>,
    /// A shell standing in for the user's. Never set outside tests.
    shell: Mutex<Option<std::path::PathBuf>>,
}

fn terminal_error(error: &TerminalError) -> HostError {
    let code = serde_json::to_value(error.code)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "internal_pty".to_owned());
    HostError {
        code,
        reasons: Vec::new(),
        bytes_written: error.bytes_written,
    }
}
fn native_error(error: &NativeError) -> HostError {
    HostError {
        code: error.code.to_owned(),
        reasons: error.reasons.clone(),
        bytes_written: 0,
    }
}
fn bad(_: serde_json::Error) -> HostError {
    HostError::new("request_invalid")
}
fn params<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, HostError> {
    serde_json::from_value(value).map_err(bad)
}
fn to_value(value: impl serde::Serialize) -> Result<Value, HostError> {
    serde_json::to_value(value).map_err(|_| HostError::new("internal"))
}
fn terminal_id(text: &str) -> Result<TerminalId, HostError> {
    TerminalId::parse(text).map_err(|_| HostError::new("request_invalid"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    size: TerminalSize,
    cwd: Option<String>,
    title: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    terminal_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Write {
    terminal_id: String,
    data: Vec<u8>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Resize {
    terminal_id: String,
    size: TerminalSize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadFrom {
    terminal_id: String,
    from: u64,
    #[serde(default)]
    wait_ms: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Id {
    id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sample {
    sample: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Restart {
    id: String,
    size: TerminalSize,
    #[serde(default)]
    plan_token: Option<String>,
    #[serde(default)]
    expected_cli_version: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Preview {
    agent_id: crate::native::catalogue::AgentId,
    profile_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Path {
    path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Default {
    project: String,
    profile_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Save {
    id: String,
    body: Body,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Restore {
    id: String,
    version: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Text {
    text: String,
}

fn title_ok(title: &str) -> bool {
    !title.trim().is_empty() && title.chars().count() <= 80 && !title.chars().any(char::is_control)
}

impl HostCore {
    pub fn new(native: Option<NativeService>) -> Arc<Self> {
        Arc::new(Self {
            terminals: TerminalService::default(),
            native,
            table: Mutex::new(HashMap::new()),
            idle_since: Mutex::new(Some(Instant::now())),
            shell: Mutex::new(None),
        })
    }

    #[cfg(test)]
    pub fn use_shell(&self, shell: std::path::PathBuf) {
        *lock(&self.shell) = Some(shell);
    }

    fn native(&self) -> Result<&NativeService, HostError> {
        self.native
            .as_ref()
            .ok_or_else(|| HostError::new("unavailable"))
    }

    /// Time since nothing (no terminal, no running agent) was held, or `None` while busy.
    pub fn idle_for(&self) -> Option<Duration> {
        let busy = !lock(&self.table).is_empty()
            || self.native.as_ref().is_some_and(NativeService::any_running);
        let mut since = lock(&self.idle_since);
        if busy {
            *since = None;
            None
        } else {
            Some(since.get_or_insert_with(Instant::now).elapsed())
        }
    }

    pub fn shutdown(&self) {
        let _ = self.terminals.shutdown();
    }

    /// Starts keeping the scrollback of a terminal that exists in the service.
    fn track(self: &Arc<Self>, info: TerminalInfo, title: String) {
        let id = info.terminal_id.clone();
        let entry = Arc::new(Entry {
            title,
            created_at: crate::native::store::now(),
            inner: Mutex::new(Inner {
                ring: Ring::default(),
                closed: false,
                eof: false,
                truncated: false,
                status: info,
            }),
            changed: Condvar::new(),
        });
        lock(&self.table).insert(id.as_str().to_ascii_lowercase(), entry.clone());
        let terminals = self.terminals.clone();
        let host = Arc::downgrade(self);
        let key = id.as_str().to_ascii_lowercase();
        let _ = std::thread::Builder::new()
            .name(format!("wyss-pump-{}", &key[..8]))
            .spawn(move || {
                pump(&terminals, &id, &entry);
                // A closed terminal is not kept; one that only exited stays until closed, so
                // its last screen can still be shown.
                if lock(&entry.inner).closed
                    && let Some(host) = host.upgrade()
                {
                    lock(&host.table).remove(&key);
                }
            });
    }

    fn entry(&self, id: &str) -> Result<Arc<Entry>, HostError> {
        lock(&self.table)
            .get(&id.to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| HostError::new("unknown_terminal"))
    }

    fn list(&self) -> Result<Value, HostError> {
        let mut rows: Vec<(u64, Value)> = Vec::new();
        for (key, entry) in lock(&self.table).iter() {
            let inner = lock(&entry.inner);
            let info = self
                .terminals
                .status(&terminal_id(key)?)
                .unwrap_or_else(|_| inner.status.clone());
            let native = self
                .native
                .as_ref()
                .and_then(|n| n.tag_of(info.terminal_id.as_str()));
            rows.push((
                entry.created_at,
                json!({
                    "terminal": info,
                    "title": entry.title,
                    "created_at": entry.created_at,
                    "native": native,
                }),
            ));
        }
        rows.sort_by_key(|(at, row)| (*at, row["terminal"]["terminal_id"].to_string()));
        Ok(Value::Array(rows.into_iter().map(|(_, row)| row).collect()))
    }

    /// One call: a closed list of operations. Anything else is `unknown_op`.
    pub fn call(self: &Arc<Self>, op: &str, value: Value) -> Result<Value, HostError> {
        match op {
            "hello" => to_value(json!({
                "protocol": super::protocol::PROTOCOL,
                "version": env!("CARGO_PKG_VERSION"),
                "pid": std::process::id(),
                "native": self.native.is_some(),
                // What this host accepts beyond protocol 1, by name, so a newer window can tell
                // an older host that is kept running across an upgrade from one that is not.
                "features": super::protocol::FEATURES,
            })),

            // ---- terminals ----
            "term_create" => {
                let request: Create = params(value)?;
                let title = request
                    .title
                    .filter(|t| title_ok(t))
                    .unwrap_or_else(|| "Terminal".to_owned());
                let shell = match lock(&self.shell).clone() {
                    Some(shell) => shell,
                    None => TerminalService::user_shell().map_err(|e| terminal_error(&e))?,
                };
                let info = self
                    .terminals
                    .create_launch(
                        CreateTerminal {
                            size: request.size,
                            cwd: request.cwd,
                        },
                        Launch {
                            program: shell,
                            args: vec!["-i".to_owned()],
                            env: Vec::new(),
                            remove_env: crate::native::service::SESSION_MARKERS
                                .iter()
                                .map(|n| (*n).to_owned())
                                .collect(),
                        },
                    )
                    .map_err(|e| terminal_error(&e))?;
                self.track(info.clone(), title);
                to_value(info)
            }
            "term_write" => {
                let request: Write = params(value)?;
                let written = self
                    .terminals
                    .write(&terminal_id(&request.terminal_id)?, request.data)
                    .map_err(|e| terminal_error(&e))?;
                to_value(written)
            }
            "term_resize" => {
                let request: Resize = params(value)?;
                to_value(
                    self.terminals
                        .resize(&terminal_id(&request.terminal_id)?, request.size)
                        .map_err(|e| terminal_error(&e))?,
                )
            }
            "term_redraw" => {
                // A program that drew its screen before a client attached gets the chance to
                // draw it again: the size is nudged by one column and put back.
                let request: Target = params(value)?;
                let id = terminal_id(&request.terminal_id)?;
                let info = self.terminals.status(&id).map_err(|e| terminal_error(&e))?;
                let nudged = TerminalSize {
                    rows: info.size.rows,
                    cols: if info.size.cols > 1 {
                        info.size.cols - 1
                    } else {
                        info.size.cols + 1
                    },
                };
                self.terminals
                    .resize(&id, nudged)
                    .map_err(|e| terminal_error(&e))?;
                std::thread::sleep(Duration::from_millis(40));
                to_value(
                    self.terminals
                        .resize(&id, info.size)
                        .map_err(|e| terminal_error(&e))?,
                )
            }
            "term_close" => {
                let request: Target = params(value)?;
                let id = terminal_id(&request.terminal_id)?;
                let closed = self.terminals.close(&id);
                // Whatever the answer, the host keeps nothing for a terminal the client closed.
                lock(&self.table).remove(&request.terminal_id.to_ascii_lowercase());
                to_value(closed.map_err(|e| terminal_error(&e))?)
            }
            "term_status" => {
                let request: Target = params(value)?;
                to_value(
                    self.terminals
                        .status(&terminal_id(&request.terminal_id)?)
                        .map_err(|e| terminal_error(&e))?,
                )
            }
            "term_read" => {
                let request: ReadFrom = params(value)?;
                self.read(&request)
            }
            "term_list" => self.list(),

            // ---- native sessions ----
            "native_launcher" => to_value(self.native()?.launcher()),
            "native_preview" => {
                let request: Preview = params(value)?;
                to_value(
                    self.native()?
                        .preview(request.agent_id, request.profile_id.as_deref())
                        .map_err(|e| native_error(&e))?,
                )
            }
            "native_start" => {
                let request: StartRequest = params(value)?;
                let reply = self
                    .native()?
                    .start(&self.terminals, request)
                    .map_err(|e| native_error(&e))?;
                self.track_native(
                    &reply.terminal,
                    &reply.session.session.agent_name,
                    &reply.session.session.project,
                );
                to_value(reply)
            }
            "native_restart" => {
                let request: Restart = params(value)?;
                let reply = self
                    .native()?
                    .restart(
                        &self.terminals,
                        &request.id,
                        request.size,
                        request.plan_token,
                        request.expected_cli_version,
                    )
                    .map_err(|e| native_error(&e))?;
                self.track_native(
                    &reply.terminal,
                    &reply.session.session.agent_name,
                    &reply.session.session.project,
                );
                to_value(reply)
            }
            "native_sessions" => {
                let request: Sample = params(value)?;
                to_value(self.native()?.sessions(&self.terminals, request.sample))
            }
            "native_stop" => {
                let request: Id = params(value)?;
                self.native()?
                    .stop(&self.terminals, &request.id)
                    .map_err(|e| native_error(&e))?;
                Ok(Value::Null)
            }
            "native_forget" => {
                let request: Id = params(value)?;
                self.native()?
                    .forget(&request.id)
                    .map_err(|e| native_error(&e))?;
                Ok(Value::Null)
            }
            "handoff_prepare" => {
                let request: Id = params(value)?;
                to_value(self.handoff_prepare(&request.id)?)
            }
            "native_remember_project" => {
                let request: Path = params(value)?;
                if !request.path.starts_with('/') || request.path.len() > 4096 {
                    return Err(HostError::new("project_invalid"));
                }
                self.native()?.remember_chosen(request.path);
                Ok(Value::Null)
            }
            "native_project_default" => {
                let request: Default = params(value)?;
                self.native()?
                    .set_project_default(&request.project, request.profile_id)
                    .map_err(|e| native_error(&e))?;
                Ok(Value::Null)
            }

            // ---- profiles ----
            "profiles" => to_value(self.native()?.profiles().list()),
            "profile_create" => {
                let body: Body = params(value)?;
                let id = self
                    .native()?
                    .profiles()
                    .create(body)
                    .map_err(profile_error)?;
                Ok(json!({ "id": id }))
            }
            "profile_save" => {
                let request: Save = params(value)?;
                let version = self
                    .native()?
                    .profiles()
                    .save(&request.id, request.body)
                    .map_err(profile_error)?;
                Ok(json!({ "version": version }))
            }
            "profile_delete" => {
                let request: Id = params(value)?;
                self.native()?
                    .profiles()
                    .delete(&request.id)
                    .map_err(profile_error)?;
                Ok(Value::Null)
            }
            "profile_duplicate" => {
                let request: Id = params(value)?;
                let id = self
                    .native()?
                    .profiles()
                    .duplicate(&request.id)
                    .map_err(profile_error)?;
                Ok(json!({ "id": id }))
            }
            "profile_restore" => {
                let request: Restore = params(value)?;
                self.native()?
                    .profiles()
                    .restore(&request.id, request.version)
                    .map_err(profile_error)?;
                Ok(Value::Null)
            }
            "profile_export" => {
                let request: Id = params(value)?;
                let bytes = self
                    .native()?
                    .profiles()
                    .export(&request.id)
                    .map_err(profile_error)?;
                Ok(json!({ "text": String::from_utf8_lossy(&bytes) }))
            }
            "profile_import" => {
                let request: Text = params(value)?;
                let id = self
                    .native()?
                    .profiles()
                    .import(request.text.as_bytes())
                    .map_err(profile_error)?;
                Ok(json!({ "id": id }))
            }
            _ => Err(HostError::new("unknown_op")),
        }
    }

    /// The brief for handing a session's work to another agent: the folder, the repository's
    /// state and the end of the screen, redacted. Nothing is started.
    fn handoff_prepare(&self, session: &str) -> Result<Value, HostError> {
        let native = self.native()?;
        let record = native
            .session_record(session)
            .ok_or_else(|| HostError::new("session_unknown"))?;
        // The screen is available while the session's terminal is still held.
        let screen = native
            .running_terminal(session)
            .and_then(|(terminal, _)| self.entry(&terminal).ok())
            .map(|entry| {
                let inner = lock(&entry.inner);
                let all = inner.ring.read(0, ring::CAPACITY);
                crate::handoff::plain_text(&all.data)
            })
            .unwrap_or_default();
        let project = std::path::PathBuf::from(&record.project);
        let text = crate::handoff::brief(
            &crate::handoff::Source {
                agent: &record.agent_name,
                project: &project,
                started_at: record.started_at,
                profile: record.profile.as_ref().map(|p| p.name.as_str()),
            },
            &screen,
        );
        Ok(json!({
            "text": text,
            "agent_id": record.agent_id,
            "project": record.project,
            "screen_included": !screen.is_empty(),
        }))
    }

    /// An agent's terminal is kept like any other, titled by what it runs.
    fn track_native(self: &Arc<Self>, info: &TerminalInfo, agent: &str, project: &str) {
        let folder = project.split('/').rfind(|p| !p.is_empty()).unwrap_or("/");
        self.track(info.clone(), format!("{agent} · {folder}"));
    }

    fn read(&self, request: &ReadFrom) -> Result<Value, HostError> {
        let entry = self.entry(&request.terminal_id)?;
        let deadline = Instant::now() + Duration::from_millis(request.wait_ms).min(MAX_WAIT);
        let mut inner = lock(&entry.inner);
        // A long read waits for something new; a short one answers at once.
        while inner.ring.end() <= request.from && !inner.eof && !inner.closed {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            inner = entry
                .changed
                .wait_timeout(inner, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        let read = inner.ring.read(request.from, MAX_READ);
        let finished = read.next >= inner.ring.end();
        Ok(json!({
            "data": read.data,
            "next": read.next,
            "truncated": read.truncated || inner.truncated,
            "eof": inner.eof && finished,
            "closed": inner.closed && finished,
            "status": inner.status,
        }))
    }
}

fn profile_error(error: crate::native::profile::ProfileError) -> HostError {
    native_error(&NativeError::from(error))
}

/// Reads one terminal's output into its ring until the terminal is closed or its process has
/// ended and been drained.
fn pump(terminals: &TerminalService, id: &TerminalId, entry: &Entry) {
    let mut ack = None;
    loop {
        match terminals.read(id, ack.as_ref()) {
            Ok(out) => {
                ack = out.batch_id.clone();
                let had = !out.data.is_empty();
                let running = matches!(
                    out.status.state,
                    wyss_console::terminal::contracts::TerminalState::Running
                        | wyss_console::terminal::contracts::TerminalState::Starting
                );
                let done = out.eof && !running && !had;
                {
                    let mut inner = lock(&entry.inner);
                    inner.ring.push(&out.data);
                    inner.truncated |= out.truncated;
                    inner.status = out.status;
                    inner.eof = done;
                }
                entry.changed.notify_all();
                if done {
                    // Its last bytes are in the ring; the reader holds nothing more.
                    return;
                }
                // Nothing came: wait for the terminal to have something (or to end), once,
                // instead of asking again every few milliseconds. A second at most, so that
                // whatever is only visible to a read (its status) is never older than that.
                if !had {
                    let _ = terminals.wait_for_output(id, Duration::from_secs(1));
                }
            }
            Err(error) => {
                let mut inner = lock(&entry.inner);
                inner.closed = error.code == ErrorCode::AlreadyClosed
                    || error.code == ErrorCode::UnknownTerminal;
                inner.eof = true;
                drop(inner);
                entry.changed.notify_all();
                return;
            }
        }
    }
}

#[allow(dead_code)]
const _RING_CAPACITY: usize = ring::CAPACITY;
