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
            json!({"terminal_id": terminal_id, "from": committed, "wait_ms": 0}),
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

const COMMANDS: &[&str] = &[
    "create_terminal",
    "terminal_write",
    "terminal_read_output",
    "terminal_resize",
    "close_terminal",
    "terminal_status",
    "terminal_redraw",
    "host_terminals",
    "host_info",
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
            terminal_redraw,
            terminal_read_output,
            host_terminals,
            host_info
        ]),
        install: Box::new(|builder| builder.manage(HostLink::new()).manage(Cursors::default())),
        on_exit: Some(Box::new(|app| {
            use tauri::Manager;
            // A window-local host ends with the window; a host process is left running.
            app.state::<HostLink>().close_local();
        })),
    }
}
