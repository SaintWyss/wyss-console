//! Client of the Wyss host protocol: a Unix socket, one connection per call, frames of a u32
//! big-endian length plus JSON. The UI uses the shared runtime socket unless a test supplies an explicit one.

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{json, Value};

use wyss_console::host::protocol::{MAX_FRAME, read_frame_body};

/// Input is sent in pieces of at most this many bytes (a byte costs up to 4 bytes of JSON).
pub const WRITE_CHUNK: usize = 64 * 1024;

/// Nanoseconds spent waiting for the host (connect, send, receive, JSON decode of the frame) and in
/// turning the answer into bytes, summed over all reads. Printed with `WYSS_CONSOLE_STATS=1`.
pub static NET_NS: AtomicU64 = AtomicU64::new(0);
pub static PARSE_NS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub enum HostError {
    Io(io::Error),
    /// The host answered `{"err": ...}`.
    Remote { code: String, reasons: Vec<String> },
    /// The answer was not the shape the protocol promises.
    Protocol(String),
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostError::Io(e) => write!(f, "io: {e}"),
            HostError::Remote { code, reasons } => write!(f, "host error {code} {reasons:?}"),
            HostError::Protocol(m) => write!(f, "protocol: {m}"),
        }
    }
}

impl From<io::Error> for HostError {
    fn from(e: io::Error) -> Self {
        HostError::Io(e)
    }
}

pub fn write_frame(w: &mut impl Write, value: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(value).map_err(io::Error::other)?;
    if body.len() > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "frame too large"));
    }
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    w.write_all(&out)
}

pub fn read_frame(r: &mut impl Read) -> io::Result<Value> {
    let body = read_frame_body(r, io::ErrorKind::InvalidData)?;
    serde_json::from_slice(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// `{"ok": v}` gives `v`; `{"err": ..}` gives `Remote`.
pub fn unwrap_answer(answer: Value) -> Result<Value, HostError> {
    if let Some(err) = answer.get("err") {
        let code = err["code"].as_str().unwrap_or("unknown").to_owned();
        let reasons = err["reasons"].as_array().map(|a| a.iter().filter_map(|r| r.as_str().map(str::to_owned)).collect()).unwrap_or_default();
        return Err(HostError::Remote { code, reasons });
    }
    match answer {
        Value::Object(mut m) if m.contains_key("ok") => Ok(m.remove("ok").unwrap()),
        _ => Err(HostError::Protocol("answer has neither ok nor err".into())),
    }
}

/// Bytes arrive as a JSON array of numbers 0-255.
fn bytes_of(v: &Value) -> Result<Vec<u8>, HostError> {
    let arr = v.as_array().ok_or_else(|| HostError::Protocol("data is not an array".into()))?;
    arr.iter().map(|n| n.as_u64().filter(|n| *n < 256).map(|n| n as u8).ok_or_else(|| HostError::Protocol("byte out of range".into()))).collect()
}

pub struct ReadOut {
    pub data: Vec<u8>,
    pub next: u64,
    pub truncated: bool,
    pub eof: bool,
    pub closed: bool,
}

pub struct Screen {
    pub at: u64,
    pub rows: u16,
    pub cols: u16,
    pub repaint: Vec<u8>,
}

pub struct ListEntry {
    pub id: String,
    pub state: String,
    /// Set when the terminal is an agent's (`NativeTag`).
    pub native: Option<crate::native::Tag>,
}

/// What `term_status` says about a terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub state: String,
    pub shell: String,
    pub cwd: String,
    pub pid: Option<u32>,
}

#[derive(Clone)]
pub struct Host {
    path: PathBuf,
}

/// An explicit test socket is never started on behalf of the caller.
pub fn connect_or_start() -> io::Result<Option<Host>> {
    if let Some(path) = std::env::var_os("WYSS_CONSOLE_HOST").filter(|path| !path.is_empty()) {
        return Ok(Some(Host::new(path)));
    }
    if std::env::var_os("WYSS_CONSOLE_CMD").is_some() {
        // A local PTY for isolated keyboard and rendering tests.
        return Ok(None);
    }
    wyss_console::host::server::check_runtime_dir()?;
    let path = wyss_console::host::server::socket_path().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "XDG_RUNTIME_DIR debe ser absoluto")
    })?;
    let executable = std::env::current_exe()?.with_file_name("wyss-host");
    start_if_needed(path, executable).map(Some)
}

fn not_our_host() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "el socket no es de nuestro host (otro usuario)")
}

fn start_if_needed(path: PathBuf, executable: PathBuf) -> io::Result<Host> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    use wyss_console::host::server::{check_socket_dir, peer_is_me};
    if let Some(dir) = path.parent() {
        check_socket_dir(dir)?;
    }
    let host = Host::new(&path);
    match UnixStream::connect(&path) {
        Ok(stream) => {
            return if peer_is_me(&stream) { Ok(host) } else { Err(not_our_host()) };
        }
        Err(error) if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) => {}
        Err(error) => return Err(error),
    }
    // Run what was verified, from the open descriptor.
    let verified = wyss_console::screen::locate::verify(&executable, nix::unistd::getuid().as_raw())
        .map_err(|why| io::Error::new(io::ErrorKind::PermissionDenied, format!("wyss-host rechazado: {why}")))?;
    let mut child = Command::new(verified.exec_path())
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()?;
    drop(verified);
    // Reap our child even when a concurrently started host wins the bind.
    std::thread::spawn(move || { let _ = child.wait(); });
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if host.call_timeout("hello", json!({}), Duration::from_millis(250)).is_ok() {
            return Ok(host);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, "wyss-host no respondió"))
}

impl Host {
    pub fn new(path: impl Into<PathBuf>) -> Host {
        Host { path: path.into() }
    }

    /// One request, one connection. `timeout` bounds each read and write on the socket.
    pub fn call_timeout(&self, op: &str, params: Value, timeout: Duration) -> Result<Value, HostError> {
        let mut s = UnixStream::connect(&self.path)?;
        if !wyss_console::host::server::peer_is_me(&s) {
            return Err(not_our_host().into());
        }
        s.set_read_timeout(Some(timeout))?;
        s.set_write_timeout(Some(timeout))?;
        write_frame(&mut s, &json!({"op": op, "params": params}))?;
        unwrap_answer(read_frame(&mut s)?)
    }

    pub fn call(&self, op: &str, params: Value) -> Result<Value, HostError> {
        self.call_timeout(op, params, Duration::from_secs(10))
    }

    pub fn hello(&self) -> Result<Value, HostError> {
        self.call("hello", json!({}))
    }

    /// Creates a terminal; returns its id.
    pub fn create(&self, rows: u16, cols: u16) -> Result<String, HostError> {
        self.create_in(rows, cols, None, None)
    }

    /// Creates a terminal in `cwd` (the host's default folder when `None`) titled `title`.
    pub fn create_in(&self, rows: u16, cols: u16, cwd: Option<&str>, title: Option<&str>) -> Result<String, HostError> {
        let v = self.call("term_create", json!({"size": {"rows": rows, "cols": cols}, "cwd": cwd, "title": title.unwrap_or("Terminal")}))?;
        v["terminal_id"].as_str().map(str::to_owned).ok_or_else(|| HostError::Protocol("no terminal_id".into()))
    }

    pub fn write(&self, id: &str, data: &[u8]) -> Result<usize, HostError> {
        let mut n = 0;
        for chunk in data.chunks(WRITE_CHUNK) {
            let v = self.call("term_write", json!({"terminal_id": id, "data": chunk}))?;
            n += v.as_u64().unwrap_or(0) as usize;
        }
        Ok(n)
    }

    /// Long-poll: answers when there is output past `from`, or after `wait_ms`.
    pub fn read(&self, id: &str, from: u64, wait_ms: u64) -> Result<ReadOut, HostError> {
        let t = Instant::now();
        let v = self.call_timeout("term_read", json!({"terminal_id": id, "from": from, "wait_ms": wait_ms}), Duration::from_millis(wait_ms) + Duration::from_secs(10))?;
        let t2 = Instant::now();
        let out = parse_read(&v);
        // Only reads that returned bytes: an empty long-poll is the host waiting, not working.
        if out.as_ref().is_ok_and(|o| !o.data.is_empty()) {
            NET_NS.fetch_add((t2 - t).as_nanos() as u64, Ordering::Relaxed);
            PARSE_NS.fetch_add(t2.elapsed().as_nanos() as u64, Ordering::Relaxed);
        }
        out
    }

    pub fn resize(&self, id: &str, rows: u16, cols: u16) -> Result<(), HostError> {
        self.call("term_resize", json!({"terminal_id": id, "size": {"rows": rows, "cols": cols}})).map(|_| ())
    }

    pub fn close(&self, id: &str) -> Result<(), HostError> {
        self.call("term_close", json!({"terminal_id": id})).map(|_| ())
    }

    pub fn list(&self) -> Result<Vec<ListEntry>, HostError> {
        parse_list(&self.call("term_list", json!({}))?)
    }

    pub fn status(&self, id: &str) -> Result<Status, HostError> {
        parse_status(&self.call("term_status", json!({"terminal_id": id}))?)
    }

    /// `None` when the host has no screen for it (parser helper missing, too large...).
    pub fn screen(&self, id: &str, scrollback_lines: u32) -> Result<Option<Screen>, HostError> {
        let v = self.call_timeout("term_screen", json!({"terminal_id": id, "scrollback_lines": scrollback_lines}), Duration::from_secs(15))?;
        parse_screen(&v)
    }
}

pub fn parse_read(v: &Value) -> Result<ReadOut, HostError> {
    Ok(ReadOut {
        data: bytes_of(&v["data"])?,
        next: v["next"].as_u64().ok_or_else(|| HostError::Protocol("no next".into()))?,
        truncated: v["truncated"].as_bool().unwrap_or(false),
        eof: v["eof"].as_bool().unwrap_or(false),
        closed: v["closed"].as_bool().unwrap_or(false),
    })
}

pub fn parse_status(v: &Value) -> Result<Status, HostError> {
    Ok(Status {
        state: v["state"].as_str().ok_or_else(|| HostError::Protocol("no state".into()))?.to_owned(),
        shell: v["shell"].as_str().unwrap_or("").to_owned(),
        cwd: v["cwd"].as_str().unwrap_or("").to_owned(),
        pid: v["observed_pid"].as_u64().and_then(|p| u32::try_from(p).ok()),
    })
}

pub fn parse_list(v: &Value) -> Result<Vec<ListEntry>, HostError> {
    let rows = v.as_array().ok_or_else(|| HostError::Protocol("list is not an array".into()))?;
    Ok(rows
        .iter()
        .filter_map(|r| {
            let t = &r["terminal"];
            Some(ListEntry { id: t["terminal_id"].as_str()?.to_owned(), state: t["state"].as_str()?.to_owned(), native: crate::native::parse_tag(&r["native"]) })
        })
        .collect())
}

pub fn parse_screen(v: &Value) -> Result<Option<Screen>, HostError> {
    if v["available"] != true {
        return Ok(None);
    }
    let side = |k: &str| v["size"][k].as_u64().filter(|n| (1..=512).contains(n)).map(|n| n as u16);
    let (Some(rows), Some(cols), Some(at), Some(b64)) = (side("rows"), side("cols"), v["at"].as_u64(), v["repaint"].as_str()) else {
        return Ok(None); // not a screen that can be drawn: the caller replays from the start
    };
    let repaint = base64::engine::general_purpose::STANDARD.decode(b64).map_err(|e| HostError::Protocol(format!("repaint: {e}")))?;
    Ok(Some(Screen { at, rows, cols, repaint }))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn frame_roundtrip_and_limits() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &json!({"op": "hello", "params": {}})).unwrap();
        assert_eq!(&buf[..4], &((buf.len() - 4) as u32).to_be_bytes());
        assert_eq!(read_frame(&mut &buf[..]).unwrap(), json!({"op": "hello", "params": {}}));
        // A length above MAX_FRAME is refused before reading the body.
        let lying = ((MAX_FRAME + 1) as u32).to_be_bytes();
        assert_eq!(read_frame(&mut &lying[..]).unwrap_err().kind(), io::ErrorKind::InvalidData);
        // A short body is an error, not a hang or a panic.
        let mut short = 10u32.to_be_bytes().to_vec();
        short.extend_from_slice(b"{}");
        assert_eq!(read_frame(&mut &short[..]).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
        // Outgoing frames over the limit are refused.
        assert!(write_frame(&mut Vec::new(), &json!("a".repeat(MAX_FRAME))).is_err());
    }

    #[test]
    fn answers_ok_and_err() {
        assert_eq!(unwrap_answer(json!({"ok": 5})).unwrap(), json!(5));
        match unwrap_answer(json!({"err": {"code": "unknown_terminal", "reasons": ["x"], "bytes_written": 0}})) {
            Err(HostError::Remote { code, reasons }) => {
                assert_eq!(code, "unknown_terminal");
                assert_eq!(reasons, vec!["x"]);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(unwrap_answer(json!({"nope": 1})), Err(HostError::Protocol(_))));
    }

    #[test]
    fn parses_status() {
        let st = parse_status(&json!({"terminal_id": "x", "state": "running", "shell": "/bin/fish", "cwd": "/home/a", "observed_pid": 4242})).unwrap();
        assert_eq!(st, Status { state: "running".into(), shell: "/bin/fish".into(), cwd: "/home/a".into(), pid: Some(4242) });
        let gone = parse_status(&json!({"state": "exited", "observed_pid": null})).unwrap();
        assert_eq!((gone.state.as_str(), gone.pid), ("exited", None));
        assert!(parse_status(&json!({})).is_err());
    }

    #[test]
    fn parses_read_list_screen() {
        let r = parse_read(&json!({"data": [104, 105, 255], "next": 7, "truncated": true, "eof": false, "closed": true, "status": {}, "parser": null})).unwrap();
        assert_eq!((r.data, r.next, r.truncated, r.eof, r.closed), (vec![104, 105, 255], 7, true, false, true));
        assert!(parse_read(&json!({"data": [256], "next": 1})).is_err());
        assert!(parse_read(&json!({"data": [1]})).is_err());

        let l = parse_list(&json!([{"terminal": {"terminal_id": "a-1", "state": "running"}, "created_at": 1}, {"title": "no terminal"}, {"terminal": {"terminal_id": "b-2", "state": "exited"}}])).unwrap();
        assert_eq!(l.iter().map(|e| (e.id.as_str(), e.state.as_str())).collect::<Vec<_>>(), vec![("a-1", "running"), ("b-2", "exited")]);
        assert!(l.iter().all(|e| e.native.is_none()));
        let agent = parse_list(&json!([{"terminal": {"terminal_id": "c-3", "state": "running"}, "native": {"session_id": "s1", "agent_name": "Codex", "project": "/home/o", "profile": "Strict", "through_shell": true}}])).unwrap();
        assert_eq!(agent[0].native.as_ref().map(|t| (t.agent_name.as_str(), t.profile.as_deref())), Some(("Codex", Some("Strict"))), "an agent's terminal carries its tag");

        let s = parse_screen(&json!({"available": true, "at": 42, "size": {"rows": 24, "cols": 80}, "repaint": "G1szMW1oaQ=="})).unwrap().unwrap();
        assert_eq!((s.at, s.rows, s.cols, s.repaint.as_slice()), (42, 24, 80, &b"\x1b[31mhi"[..]));
        assert!(parse_screen(&json!({"available": false, "reason": "helper_missing"})).unwrap().is_none());
        assert!(parse_screen(&json!({"available": true, "at": 1, "size": {"rows": 0, "cols": 80}, "repaint": ""})).unwrap().is_none());
    }

    /// A fake host: answers each connection with the recorded frame for its op and records the request.
    pub(crate) fn fake_host(answers: Vec<(&'static str, Value)>) -> (PathBuf, std::thread::JoinHandle<Vec<Value>>) {
        let path = std::env::temp_dir().join(format!("wyss-fake-{}-{:?}.sock", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let p = path.clone();
        let h = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for (op, answer) in answers {
                let (mut s, _) = listener.accept().unwrap();
                let req = read_frame(&mut s).unwrap();
                assert_eq!(req["op"], op);
                write_frame(&mut s, &answer).unwrap();
                seen.push(req);
            }
            let _ = std::fs::remove_file(p);
            seen
        });
        (path, h)
    }

    #[test]
    fn client_against_a_fake_host() {
        let (path, h) = fake_host(vec![
            ("hello", json!({"ok": {"features": ["a"]}})),
            ("term_create", json!({"ok": {"terminal_id": "t-1", "state": "running"}})),
            ("term_write", json!({"ok": 3})),
            ("term_read", json!({"ok": {"data": [104, 105], "next": 2, "truncated": false, "eof": false, "closed": false}})),
            ("term_resize", json!({"ok": null})),
            ("term_close", json!({"err": {"code": "already_closed", "reasons": [], "bytes_written": 0}})),
        ]);
        let host = Host::new(&path);
        assert_eq!(host.hello().unwrap()["features"][0], "a");
        assert_eq!(host.create(24, 80).unwrap(), "t-1");
        assert_eq!(host.write("t-1", b"abc").unwrap(), 3);
        let r = host.read("t-1", 0, 50).unwrap();
        assert_eq!((r.data.as_slice(), r.next), (&b"hi"[..], 2));
        host.resize("t-1", 30, 100).unwrap();
        assert!(matches!(host.close("t-1"), Err(HostError::Remote { ref code, .. }) if code == "already_closed"));
        let seen = h.join().unwrap();
        assert_eq!(seen[1]["params"]["size"], json!({"rows": 24, "cols": 80}));
        assert_eq!(seen[2]["params"]["data"], json!([97, 98, 99]));
        assert_eq!(seen[3]["params"]["from"], 0);
        assert_eq!(seen[4]["params"]["size"], json!({"rows": 30, "cols": 100}));
    }

    /// A fresh directory with the given mode, under the scratch tree.
    fn dir_with(mode: u32) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("wh-dir-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(mode)).unwrap();
        dir
    }

    #[test]
    fn startup_reuses_a_live_socket_without_launching_a_binary() {
        let dir = dir_with(0o700);
        let path = dir.join("host.sock");
        let listener = UnixListener::bind(&path).unwrap();
        // A nonexistent executable makes any accidental attempt to spawn fail.
        let host = start_if_needed(path.clone(), path.with_extension("missing")).unwrap();
        assert_eq!(host.path, path);
        let _ = listener.accept().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_socket_in_a_directory_open_to_others_is_not_trusted() {
        let dir = dir_with(0o777);
        let path = dir.join("host.sock");
        let _listener = UnixListener::bind(&path).unwrap();
        let error = start_if_needed(path, dir.join("missing")).err().expect("must refuse");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_sibling_host_binary_writable_by_others_is_not_run() {
        use std::os::unix::fs::PermissionsExt;
        let dir = dir_with(0o700);
        let exe = dir.join("wyss-host");
        std::fs::write(&exe, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o777)).unwrap();
        // Nothing listens, so the only way forward would be to run the binary.
        let error = start_if_needed(dir.join("host.sock"), exe).err().expect("must refuse");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn connecting_to_nothing_is_an_io_error() {
        let host = Host::new("/nonexistent/wyss-host.sock");
        assert!(matches!(host.hello(), Err(HostError::Io(_))));
    }

    /// Against a real host started in an isolated HOME/XDG tree (never the user's host):
    /// Cargo builds the sibling `wyss-host` for the isolated tests.
    #[test]
    fn isolated_host_roundtrip() {
        let iso = IsolatedHost::start();
        let host = Host::new(&iso.sock);
        assert!(host.hello().unwrap()["features"].is_array());
        let id = host.create(24, 80).unwrap();
        host.write(&id, b"echo hola-host\n").unwrap();
        let (mut from, mut text) = (0u64, Vec::new());
        for _ in 0..20 {
            let r = host.read(&id, from, 500).unwrap();
            from = r.next;
            text.extend_from_slice(&r.data);
            if String::from_utf8_lossy(&text).contains("hola-host") {
                break;
            }
        }
        assert!(String::from_utf8_lossy(&text).contains("hola-host"), "{:?}", String::from_utf8_lossy(&text));
        assert!(host.list().unwrap().iter().any(|e| e.id == id && e.state == "running"));
        // What the window shows about the terminal: its process, memory and folder.
        let st = host.status(&id).unwrap();
        assert_eq!(st.state, "running");
        let pid = st.pid.expect("observed_pid") as i32;
        assert!(crate::procinfo::tree_rss(pid).unwrap() > 0);
        assert!(crate::procinfo::cwd(pid).is_some());
        // The parser helper may need a moment to have the screen.
        let mut screen = None;
        for _ in 0..20 {
            screen = host.screen(&id, 100).unwrap();
            if screen.as_ref().is_some_and(|s| String::from_utf8_lossy(&s.repaint).contains("hola-host")) {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        let s = screen.expect("screen available");
        assert!(String::from_utf8_lossy(&s.repaint).contains("hola-host"));
        host.close(&id).unwrap();
        assert!(!host.list().unwrap().iter().any(|e| e.id == id));
    }

    /// The launch flow against a real host in an isolated tree: the harmless preset (its only command is
    /// `echo preset-ok`) is written by this test, the folder is read, the home and the previews of the
    /// agents are asked of the host, and the preset is opened and read back. Nothing here ever starts an
    /// agent: `native_start` is exercised only against recorded answers (`native.rs`).
    #[test]
    fn isolated_launch_flow() {
        use crate::launches::{self, Kind, System};
        use crate::native::{self, Opened};
        let iso = IsolatedHost::start();
        let host = Host::new(&iso.sock);
        let folder = iso.dir.join("config/wyss-console/launch");
        std::fs::create_dir_all(&folder).unwrap();
        // The host seeds nothing, but whatever is in the folder is cleared before the first call.
        for e in std::fs::read_dir(&folder).unwrap().flatten() {
            std::fs::remove_file(e.path()).unwrap();
        }
        std::fs::write(folder.join("preset-inofensivo.toml"), "name = \"Preset inofensivo\"\ndirectory = \"~\"\ncommands = [\"echo preset-ok\"]\n").unwrap();
        std::fs::write(folder.join("codex-prueba.toml"), "name = \"Codex de prueba\"\ndirectory = \"~\"\n[session]\nagent = \"codex\"\npermission = \"bypass\"\n").unwrap();

        let listing = launches::read_folder(&folder, &System::read(std::path::Path::new("/etc/os-release")));
        assert!(listing.refused.is_empty(), "{:?}", listing.refused);
        let names: Vec<(&str, &str, bool)> = listing.launches.iter().map(|l| (l.name.as_str(), l.type_name(), l.full_access())).collect();
        assert_eq!(names, vec![("Codex de prueba", "Codex", true), ("Preset inofensivo", "Terminal", false)]);

        // native_launcher: the home is the isolated one; native_preview of an agent is a check, not a start.
        let launcher = native::launcher(&host).unwrap();
        let home = launcher.home.clone().expect("the host knows its home project");
        assert!(home.starts_with(iso.dir.to_str().unwrap()), "{home}");
        if launcher.installed.iter().any(|a| a == "codex") {
            let p = native::preview(&host, "codex", Some("template-full-access")).unwrap();
            assert!(!p.can_start || p.plan_token.is_some(), "a start that may go on is bound to its token");
        }

        // The only launch ever opened here: the one this test wrote, and only if it is exactly what it says.
        let harmless = launches::get(&folder, "preset-inofensivo", &System::read(std::path::Path::new("/etc/os-release"))).unwrap();
        assert_eq!(harmless.kind, Kind::Commands { commands: vec!["echo preset-ok".into()] });
        let (_, print) = launches::get_with_fingerprint(&folder, "preset-inofensivo", &System::read(std::path::Path::new("/etc/os-release"))).unwrap();
        let approvals = wyss_console::launch::Approvals::at(iso.dir.join("state/launch-approvals.json"));
        let needed = match native::open_launch(&host, &harmless, &print, &home, &native::run_before, Some(&approvals)).unwrap() {
            Opened::NeedsApproval(needed) => needed,
            other => panic!("a file nobody approved types nothing: {other:?}"),
        };
        approvals.approve(&needed.fingerprint).unwrap();
        let opened = native::open_launch(&host, &harmless, &print, &home, &native::run_before, Some(&approvals)).unwrap();
        let Opened::Terminal { terminal_id, title } = opened else { panic!("a commands launch opens a terminal") };
        assert_eq!(title, "Preset inofensivo");
        // fish waits 10 s for a terminal that answers its queries before it reads the typed line.
        let (mut from, mut text) = (0u64, Vec::new());
        for _ in 0..100 {
            let r = host.read(&terminal_id, from, 500).unwrap();
            from = r.next;
            text.extend_from_slice(&r.data);
            if String::from_utf8_lossy(&text).contains("\rpreset-ok\r\n") {
                break;
            }
        }
        let shown = String::from_utf8_lossy(&text).into_owned();
        assert!(shown.contains("\rpreset-ok\r\n"), "the command ran (its output, not only its echo): {shown:?}");
        assert!(host.status(&terminal_id).unwrap().cwd.starts_with(&home), "it opened in the launch's folder");
        // No agent was started: the registry has no session.
        assert!(native::sessions(&host).unwrap().is_empty());
        host.close(&terminal_id).unwrap();
        assert!(!host.list().unwrap().iter().any(|e| e.id == terminal_id));
    }

    struct IsolatedHost {
        child: std::process::Child,
        sock: PathBuf,
        dir: PathBuf,
    }

    impl IsolatedHost {
        fn start() -> IsolatedHost {
            use std::os::unix::fs::DirBuilderExt;
            // One tree per host: tests of this process run in parallel.
            static N: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!("wh-t{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
            for sub in ["", "run", "home", "state", "data", "config", "cache"] {
                std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir.join(sub)).unwrap();
            }
            let child = std::process::Command::new(std::env::current_exe().unwrap().parent().unwrap().join("../wyss-host"))
                .env_clear()
                .env("PATH", std::env::var("PATH").unwrap_or_default())
                .env("HOME", dir.join("home"))
                .env("SHELL", "/bin/sh")
                .env("XDG_RUNTIME_DIR", dir.join("run"))
                .env("XDG_STATE_HOME", dir.join("state"))
                .env("XDG_DATA_HOME", dir.join("data"))
                .env("XDG_CONFIG_HOME", dir.join("config"))
                .env("XDG_CACHE_HOME", dir.join("cache"))
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("start isolated host");
            let sock = dir.join("run/wyss-console/host.sock");
            for _ in 0..100 {
                if sock.exists() {
                    return IsolatedHost { child, sock, dir };
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            drop(IsolatedHost { child, sock, dir });
            panic!("isolated host socket never appeared");
        }
    }

    impl Drop for IsolatedHost {
        fn drop(&mut self) {
            let _ = self.child.kill(); // our own child only
            let _ = self.child.wait();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}
