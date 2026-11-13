//! The window's terminal commands, served through the host (F9). They have the names and the
//! shapes the window always used (`create_terminal`, `terminal_read_output`…), so the page did
//! not change to move its terminals out of its own process; this file answers them by asking
//! the host. Reading keeps the window's credit rule: a batch is delivered once the previous
//! one is acknowledged; the host keeps the scrollback, so a window that opens later starts at
//! the beginning of what is held.
use super::{client::HostLink, protocol::HostError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};
use tauri::State;
use wyss_console::terminal::contracts::TerminalSize;

/// What the window gets for a failed terminal command: the shape `TerminalError` always had.
#[derive(Debug, Serialize)]
pub struct WireError {
    code: String,
    bytes_written: usize,
}
impl From<HostError> for WireError {
    fn from(error: HostError) -> Self {
        Self {
            code: error.code,
            bytes_written: error.bytes_written,
        }
    }
}
fn invalid() -> WireError {
    WireError {
        code: "invalid_input".to_owned(),
        bytes_written: 0,
    }
}

#[derive(Default)]
pub struct Cursors {
    by_terminal: Mutex<HashMap<String, Cursor>>,
}
#[derive(Default)]
struct Cursor {
    committed: u64,
    pending: Option<(String, u64)>,
    /// A screen handed to the window that it has not yet said it drew: (token, where it ends).
    staged: Option<(String, u64)>,
}

impl Cursor {
    /// A screen ending at `at` is being handed to the window; reading does not move yet.
    fn stage(&mut self, at: u64) -> String {
        let token = uuid::Uuid::new_v4().to_string();
        self.pending = None;
        self.staged = Some((token.clone(), at));
        token
    }
    /// No screen was handed out: what the host kept is read from its start.
    fn replay(&mut self) {
        self.pending = None;
        self.staged = None;
        self.committed = 0;
    }
    /// The window says whether it drew the staged screen. `false`: the token was not the one
    /// handed out last (nothing changes, the staged screen stays).
    fn commit(&mut self, token: &str, restored: bool) -> bool {
        match self.staged.take() {
            Some((staged, at)) if staged == token => {
                self.pending = None;
                self.committed = if restored { at } else { 0 };
                true
            }
            other => {
                self.staged = other;
                false
            }
        }
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, HostError> + Send + 'static,
) -> Result<T, WireError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| WireError {
            code: "internal_pty".to_owned(),
            bytes_written: 0,
        })?
        .map_err(WireError::from)
}

fn id_ok(id: &str) -> bool {
    id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateRequest {
    size: TerminalSize,
    cwd: Option<String>,
    title: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetRequest {
    terminal_id: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteRequest {
    terminal_id: String,
    data: Vec<u8>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResizeRequest {
    terminal_id: String,
    size: TerminalSize,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadRequest {
    terminal_id: String,
    ack: Option<String>,
}

#[tauri::command]
pub async fn create_terminal(
    link: State<'_, HostLink>,
    request: CreateRequest,
) -> Result<Value, WireError> {
    let link = link.inner().clone();
    blocking(move || {
        link.call(
            "term_create",
            json!({"size": request.size, "cwd": request.cwd, "title": request.title}),
        )
    })
    .await
}

#[tauri::command]
pub async fn terminal_write(
    link: State<'_, HostLink>,
    request: WriteRequest,
) -> Result<Value, WireError> {
    if !id_ok(&request.terminal_id) {
        return Err(invalid());
    }
    let link = link.inner().clone();
    blocking(move || {
        link.call(
            "term_write",
            json!({"terminal_id": request.terminal_id, "data": request.data}),
        )
    })
    .await
}

#[tauri::command]
pub async fn terminal_resize(
    link: State<'_, HostLink>,
    request: ResizeRequest,
) -> Result<Value, WireError> {
    if !id_ok(&request.terminal_id) {
        return Err(invalid());
    }
    let link = link.inner().clone();
    blocking(move || {
        link.call(
            "term_resize",
            json!({"terminal_id": request.terminal_id, "size": request.size}),
        )
    })
    .await
}

#[tauri::command]
pub async fn close_terminal(
    link: State<'_, HostLink>,
    cursors: State<'_, Cursors>,
    request: TargetRequest,
) -> Result<Value, WireError> {
    if !id_ok(&request.terminal_id) {
        return Err(invalid());
    }
    cursors
        .by_terminal
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&request.terminal_id.to_ascii_lowercase());
    let link = link.inner().clone();
    blocking(move || link.call("term_close", json!({"terminal_id": request.terminal_id}))).await
}

#[tauri::command]
pub async fn terminal_status(
    link: State<'_, HostLink>,
    request: TargetRequest,
) -> Result<Value, WireError> {
    if !id_ok(&request.terminal_id) {
        return Err(invalid());
    }
    let link = link.inner().clone();
    blocking(move || link.call("term_status", json!({"terminal_id": request.terminal_id}))).await
}

/// The git state of a session's folder (session bar). The folder is the one the host holds for
/// that terminal, never a path from the window: the window can only name a terminal it has.
#[tauri::command]
pub async fn terminal_git_status(
    link: State<'_, HostLink>,
    request: TargetRequest,
) -> Result<Value, WireError> {
    if !id_ok(&request.terminal_id) {
        return Err(invalid());
    }
    let link = link.inner().clone();
    blocking(move || {
        let info = link.call("term_status", json!({"terminal_id": request.terminal_id}))?;
        let started = info
            .get("cwd")
            .and_then(Value::as_str)
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_absolute());
        // Where the shell is now, after a `cd`: the folder of the process the host started,
        // read from /proc. A process that ended or cannot be read falls back to where it started.
        let now = info
            .get("observed_pid")
            .and_then(Value::as_u64)
            .filter(|_| info.get("state").and_then(Value::as_str) == Some("running"))
            .and_then(|pid| std::fs::read_link(format!("/proc/{pid}/cwd")).ok())
            .filter(|p| p.is_absolute() && p.is_dir());
        let Some(dir) = now.or(started) else {
            return Ok(json!({"kind": "unknown", "dir": null}));
        };
        let mut reply = match wyss_console::git_status::status(&dir) {
            Ok(status) => serde_json::to_value(status).unwrap_or(json!({"kind": "unknown"})),
            // A git that ran past its limit is said, so the window waits longer to ask again.
            Err(wyss_console::git_status::GitError::Timeout) => {
                json!({"kind": "unknown", "slow": true})
            }
            Err(_) => json!({"kind": "unknown"}),
        };
        reply["dir"] = json!(dir.to_string_lossy());
        // For reading the folder as ~/…; the window never uses it as a path.
        reply["home"] = json!(std::env::var("HOME").ok().filter(|h| h.starts_with('/')));
        Ok(reply)
    })
    .await
}

/// A program that drew its screen before this window attached draws it again.
#[tauri::command]
pub async fn terminal_redraw(
    link: State<'_, HostLink>,
    request: TargetRequest,
) -> Result<Value, WireError> {
    if !id_ok(&request.terminal_id) {
        return Err(invalid());
    }
    let link = link.inner().clone();
    blocking(move || link.call("term_redraw", json!({"terminal_id": request.terminal_id}))).await
}

/// A window attaches to a terminal that kept running without it: the host's parser helper has the
/// screen, and the answer is the bytes that redraw it plus where in the output they end, so that
/// the next read starts there and nothing is drawn twice or missed. A host that has no parser (an
/// older one kept running across an upgrade, or one without its helper) answers `replay`, and the
/// window does what it always did: reads what the host kept from the start.
#[tauri::command]
pub async fn terminal_attach(
    link: State<'_, HostLink>,
    cursors: State<'_, Cursors>,
    request: TargetRequest,
) -> Result<Value, WireError> {
    if !id_ok(&request.terminal_id) {
        return Err(invalid());
    }
    let key = request.terminal_id.to_ascii_lowercase();
    let link = link.inner().clone();
    let terminal_id = request.terminal_id.clone();
    let answer = blocking(move || {
        match link.call(
            "term_screen",
            json!({"terminal_id": terminal_id, "scrollback_lines": 500}),
        ) {
            Err(error) if error.code == "unknown_op" => Ok(json!({
                "available": false,
                "reason": "host_without_screen",
                "parser": null,
            })),
            other => other,
        }
    })
    .await?;
    let mut map = cursors
        .by_terminal
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let cursor = map.entry(key).or_default();
    if answer["available"] == true && screen_answer_is_sound(&answer) {
        // The read position does not move yet: it moves when the window says it drew this screen
        // (`terminal_attach_commit`), so a screen that could not be drawn loses nothing.
        let token = cursor.stage(answer["at"].as_u64().unwrap_or(0));
        let mut out = answer;
        out["mode"] = json!("screen");
        out["token"] = json!(token);
        Ok(out)
    } else {
        cursor.replay();
        let reason = if answer["available"] == true {
            json!("invalid_screen")
        } else {
            answer["reason"].clone()
        };
        Ok(json!({
            "mode": "replay",
            "reason": reason,
            "parser": answer["parser"],
        }))
    }
}

/// What the window is told to draw has to be something it can draw: a size inside the bounds, a
/// repaint inside its bound, an offset. Anything else is replayed instead, before any state moves.
fn screen_answer_is_sound(answer: &Value) -> bool {
    let size_ok = |key: &str| {
        answer["size"][key]
            .as_u64()
            .is_some_and(|n| (1..=MAX_SCREEN_SIDE).contains(&n))
    };
    answer["repaint"]
        .as_str()
        .is_some_and(|r| r.len() <= MAX_REPAINT_BASE64)
        && answer["at"].is_u64()
        && size_ok("rows")
        && size_ok("cols")
}

/// The most a screen can be on a side, and the most base64 of a repaint a window is given.
const MAX_SCREEN_SIDE: u64 = 512;
const MAX_REPAINT_BASE64: usize = 900 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitRequest {
    terminal_id: String,
    /// The token the attach answer carried; none when the answer could not be read at all, which
    /// can only mean `restored: false`.
    token: Option<String>,
    /// The window drew the screen it was given. When it could not, reading starts from the
    /// beginning of what the host kept, as before.
    restored: bool,
}

/// The second half of an attach: the window says whether it drew the screen it was given, and only
/// then does the read position move. A token that is not the one handed out last is refused.
#[tauri::command]
pub async fn terminal_attach_commit(
    cursors: State<'_, Cursors>,
    request: CommitRequest,
) -> Result<Value, WireError> {
    if !id_ok(&request.terminal_id) || request.token.as_deref().is_some_and(|t| t.len() > 64) {
        return Err(invalid());
    }
    if request.token.is_none() && request.restored {
        return Err(invalid());
    }
    let key = request.terminal_id.to_ascii_lowercase();
    let mut map = cursors
        .by_terminal
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let cursor = map.entry(key).or_default();
    let Some(token) = request.token.as_deref() else {
        // An answer that could not be read: whatever was staged is dropped and the output the host
        // kept is read from its start.
        cursor.replay();
        return Ok(json!({"committed": false}));
    };
    if cursor.commit(token, request.restored) {
        Ok(json!({"committed": request.restored}))
    } else {
        Err(WireError {
            code: "invalid_attach_token".to_owned(),
            bytes_written: 0,
        })
    }
}

/// What survives what, from facts read on this machine, and whether lingering is on.
#[tauri::command]
pub async fn host_persistence(link: State<'_, HostLink>) -> Result<Value, WireError> {
    let link = link.inner().clone();
    blocking(move || link.call("host_persistence", Value::Null)).await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LingeringRequest {
    enable: bool,
    confirm: String,
}

/// Turns lingering on or off: the owner's explicit action, with its confirmation word. Never
/// called by anything but the button that asks for it.
#[tauri::command]
pub async fn host_lingering(
    link: State<'_, HostLink>,
    request: LingeringRequest,
) -> Result<Value, WireError> {
    if std::env::var_os("WYSS_GUI_ONLY_ROOT").is_some() {
        return Err(invalid());
    }
    if request.confirm.len() > 32 {
        return Err(invalid());
    }
    let link = link.inner().clone();
    blocking(move || {
        link.call(
            "host_lingering",
            json!({"enable": request.enable, "confirm": request.confirm}),
        )
    })
    .await
}

/// How long a read that finds nothing new waits for something before it answers empty. The page
/// used to ask again every 320 ms at rest, three times a second per terminal, through the webview,
/// the desktop and the host; now it asks once and is answered when there is output, or after this
/// (P1). Below the host's own bound, and well below the call timeout.
const LONG_POLL_MS: u64 = 1000;

#[tauri::command]
pub async fn terminal_read_output(
    link: State<'_, HostLink>,
    cursors: State<'_, Cursors>,
    request: ReadRequest,
) -> Result<Value, WireError> {
    if !id_ok(&request.terminal_id) {
        return Err(invalid());
    }
    let key = request.terminal_id.to_ascii_lowercase();
    let committed = {
        let mut map = cursors
            .by_terminal
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let cursor = map.entry(key.clone()).or_default();
        if let Some(ack) = &request.ack {
            match cursor.pending.take() {
                Some((batch, next)) if &batch == ack => cursor.committed = next,
                other => {
                    cursor.pending = other;
                    return Err(WireError {
                        code: "invalid_ack".to_owned(),
                        bytes_written: 0,
                    });
                }
            }
        }
        cursor.committed
    };
    let link = link.inner().clone();
    let terminal_id = request.terminal_id.clone();
    let answer = blocking(move || {
        link.call(
            "term_read",
            json!({"terminal_id": terminal_id, "from": committed, "wait_ms": LONG_POLL_MS}),
        )
    })
    .await?;
    let data = answer["data"].clone();
    let has_data = data.as_array().is_some_and(|d| !d.is_empty());
    let next = answer["next"].as_u64().unwrap_or(committed);
    let batch_id = has_data.then(|| uuid::Uuid::new_v4().to_string());
    if let Some(batch) = &batch_id {
        cursors
            .by_terminal
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(key)
            .or_default()
            .pending = Some((batch.clone(), next));
    }
    Ok(json!({
        "batch_id": batch_id,
        "data": data,
        "eof": answer["eof"],
        "truncated": answer["truncated"],
        "status": answer["status"],
        "parser": answer["parser"],
    }))
}

/// The terminals the host holds, so that a window that opens later can show them again.
#[tauri::command]
pub async fn host_terminals(link: State<'_, HostLink>) -> Result<Value, WireError> {
    let link = link.inner().clone();
    blocking(move || link.call("term_list", Value::Null)).await
}

/// Whether the terminals live in a host outside this window (and so outlive it).
#[tauri::command]
pub async fn host_info(link: State<'_, HostLink>) -> Result<Value, WireError> {
    let link = link.inner().clone();
    blocking(move || {
        let remote = link.is_remote();
        Ok(json!({ "remote": remote }))
    })
    .await
}

// ---- the layout of tabs and panes (T2) -------------------------------------------------------

/// Most bytes of a saved layout. Sixteen tabs of sixteen panes with their titles fit in a few
/// kilobytes; anything near this bound is not a layout.
const MAX_LAYOUT: usize = 64 * 1024;

fn layout_path() -> Option<std::path::PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state"))
        })?;
    Some(state.join("wyss-console").join("layout.json"))
}

/// The saved layout, or null. The window checks its shape; the desktop only keeps it.
#[tauri::command]
pub fn layout_read() -> Value {
    layout_path()
        .and_then(|path| crate::native::store::read_bounded(&path, MAX_LAYOUT as u64))
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(Value::Null)
}

/// Keeps the layout: a JSON object of bounded size, written atomically (0600) in the
/// Console's own state directory. Nothing in it is ever run.
#[tauri::command]
pub fn layout_write(layout: Value) -> Result<(), WireError> {
    let bytes = serde_json::to_vec(&layout).map_err(|_| invalid())?;
    if !layout.is_object() || bytes.len() > MAX_LAYOUT {
        return Err(invalid());
    }
    let path = layout_path().ok_or_else(invalid)?;
    let dir = path.parent().ok_or_else(invalid)?;
    crate::native::store::private_dir(dir).map_err(|_| invalid())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    // A layout a newer Console wrote is left as it is; an unreadable one is kept aside.
    super::layout::write(&path, &bytes, now).map_err(|refused| match refused {
        super::layout::Refused::Newer => WireError {
            code: "layout_newer".to_owned(),
            bytes_written: 0,
        },
        super::layout::Refused::Io => invalid(),
    })
}

const COMMANDS: &[&str] = &[
    "create_terminal",
    "terminal_write",
    "terminal_read_output",
    "terminal_resize",
    "close_terminal",
    "terminal_status",
    "terminal_git_status",
    "terminal_redraw",
    "terminal_attach",
    "terminal_attach_commit",
    "host_persistence",
    "host_lingering",
    "host_terminals",
    "host_info",
    "layout_read",
    "layout_write",
];

pub fn extension() -> wyss_console::Extension {
    wyss_console::Extension {
        commands: COMMANDS,
        handler: Box::new(tauri::generate_handler![
            create_terminal,
            terminal_write,
            terminal_resize,
            close_terminal,
            terminal_status,
            terminal_git_status,
            terminal_redraw,
            terminal_attach,
            terminal_attach_commit,
            host_persistence,
            host_lingering,
            terminal_read_output,
            host_terminals,
            host_info,
            layout_read,
            layout_write
        ]),
        install: Box::new(|builder| builder.manage(HostLink::new()).manage(Cursors::default())),
        on_exit: Some(Box::new(|app| {
            use tauri::Manager;
            // A window-local host ends with the window; a host process is left running.
            app.state::<HostLink>().close_local();
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_read_position_moves_only_when_the_window_says_it_drew_the_screen() {
        let mut cursor = Cursor {
            committed: 7,
            ..Cursor::default()
        };
        let token = cursor.stage(900);
        assert_eq!(cursor.committed, 7, "staging moves nothing");
        assert!(cursor.commit(&token, true));
        assert_eq!(cursor.committed, 900);
        // Used once: the same token again is refused and moves nothing.
        assert!(!cursor.commit(&token, true));
        assert_eq!(cursor.committed, 900);
    }

    #[test]
    fn a_screen_that_could_not_be_drawn_sends_the_reader_back_to_the_start() {
        let mut cursor = Cursor {
            committed: 500,
            ..Cursor::default()
        };
        let token = cursor.stage(900);
        assert!(cursor.commit(&token, false));
        assert_eq!(
            cursor.committed, 0,
            "the output the host kept is read from its start"
        );
    }

    #[test]
    fn a_token_that_is_not_the_last_one_is_refused_and_the_staged_screen_stays() {
        let mut cursor = Cursor::default();
        let first = cursor.stage(10);
        let second = cursor.stage(20);
        assert!(!cursor.commit(&first, true));
        assert!(!cursor.commit("not a token", true));
        assert_eq!(cursor.committed, 0);
        assert!(cursor.commit(&second, true));
        assert_eq!(cursor.committed, 20);
    }

    #[test]
    fn an_unusable_screen_is_not_handed_out() {
        let good = json!({"repaint": "AAAA", "at": 5, "size": {"rows": 24, "cols": 80}});
        assert!(screen_answer_is_sound(&good));
        for bad in [
            json!({"repaint": "AAAA", "at": 5, "size": {"rows": 0, "cols": 80}}),
            json!({"repaint": "AAAA", "at": 5, "size": {"rows": 24, "cols": 513}}),
            json!({"repaint": "AAAA", "at": -1, "size": {"rows": 24, "cols": 80}}),
            json!({"repaint": "AAAA", "size": {"rows": 24, "cols": 80}}),
            json!({"repaint": 7, "at": 5, "size": {"rows": 24, "cols": 80}}),
            json!({"repaint": "A".repeat(MAX_REPAINT_BASE64 + 1), "at": 5, "size": {"rows": 24, "cols": 80}}),
        ] {
            assert!(!screen_answer_is_sound(&bad), "{bad}");
        }
    }
}
