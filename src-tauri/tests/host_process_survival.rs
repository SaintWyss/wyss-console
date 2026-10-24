//! M4: the host as a real process, in a runtime directory, a home and a state directory made for
//! the test. What it shows is what a terminal survives and what it does not: the window leaving
//! (no window here: a client that comes and goes), a host that is killed, and a new host after it.
//! Nothing of the owner's runtime directory, host, shell configuration or state is touched.

use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    os::unix::{fs::DirBuilderExt, net::UnixStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Sandbox {
    root: PathBuf,
}
impl Sandbox {
    fn new() -> Self {
        let root = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("wyss-survival-{}", uuid::Uuid::new_v4().simple()));
        for dir in ["run", "home", "state", "config", "data"] {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(root.join(dir))
                .unwrap();
        }
        Self { root }
    }
    fn socket(&self) -> PathBuf {
        self.root.join("run/wyss-console/host.sock")
    }
    fn start_host(&self) -> Child {
        Command::new(env!("CARGO_BIN_EXE_wyss-console"))
            .arg("--wyss-host")
            .env_clear()
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("HOME", self.root.join("home"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C.UTF-8")
            .current_dir("/")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the host starts")
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn call(socket: &Path, op: &str, params: Value) -> Result<Value, Value> {
    let mut stream = UnixStream::connect(socket).map_err(|e| json!({"code": e.to_string()}))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    let body = serde_json::to_vec(&json!({"op": op, "params": params})).unwrap();
    stream
        .write_all(&(body.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(&body).unwrap();
    let mut len = [0u8; 4];
    stream
        .read_exact(&mut len)
        .map_err(|e| json!({"code": e.to_string()}))?;
    let mut answer = vec![0u8; u32::from_be_bytes(len) as usize];
    stream.read_exact(&mut answer).unwrap();
    let value: Value = serde_json::from_slice(&answer).unwrap();
    if let Some(ok) = value.get("ok") {
        Ok(ok.clone())
    } else {
        Err(value["err"].clone())
    }
}

fn wait_for_socket(sandbox: &Sandbox) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if call(&sandbox.socket(), "hello", Value::Null).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("the host never answered");
}

fn alive(pid: u64) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => !stat.contains(") Z") && !stat.contains(") X"),
        Err(_) => false,
    }
}

fn read_all(socket: &Path, id: &str, needle: &str) -> bool {
    let deadline = Instant::now() + Duration::from_secs(40);
    let mut from = 0u64;
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        let Ok(out) = call(
            socket,
            "term_read",
            json!({"terminal_id": id, "from": from, "wait_ms": 300}),
        ) else {
            return false;
        };
        from = out["next"].as_u64().unwrap();
        seen.extend(
            out["data"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| b.as_u64().unwrap() as u8),
        );
        if String::from_utf8_lossy(&seen).contains(needle) {
            return true;
        }
    }
    false
}

#[test]
fn a_terminal_survives_every_client_and_nothing_survives_the_host_nor_comes_back_by_itself() {
    let sandbox = Sandbox::new();
    let mut host = sandbox.start_host();
    wait_for_socket(&sandbox);
    let socket = sandbox.socket();

    // A client makes a terminal and leaves.
    let made = call(
        &socket,
        "term_create",
        json!({"size": {"rows": 24, "cols": 80}, "title": "T"}),
    )
    .unwrap();
    let id = made["terminal_id"].as_str().unwrap().to_owned();
    let pid = made["observed_pid"].as_u64().unwrap();
    // The account shell is replaced by a long sleep, so that "the program" is one process we know.
    let line = b"exec sleep 600\r".to_vec();
    call(
        &socket,
        "term_write",
        json!({"terminal_id": id, "data": line}),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(40);
    while std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|c| c.trim() != "sleep")
        .unwrap_or(true)
    {
        assert!(
            Instant::now() < deadline,
            "the shell never became the sleep"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    // Clients come and go: the program does not notice.
    for _ in 0..3 {
        let listed = call(&socket, "term_list", Value::Null).unwrap();
        assert_eq!(listed.as_array().unwrap().len(), 1);
    }
    assert!(alive(pid), "the program outlives every client");
    let before = call(&socket, "term_status", json!({"terminal_id": id})).unwrap();
    assert_eq!(before["state"], "running");

    // The terminal's parser helper (when this build has one) is a child of the host.
    let helper =
        call(&socket, "term_parser", json!({"terminal_id": id})).unwrap()["helper_pid"].as_u64();

    // The host is killed the way a crash or an out-of-memory killer ends it.
    host.kill().unwrap();
    host.wait().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while alive(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !alive(pid),
        "the host owned the PTY: with it gone, the program is hung up"
    );
    assert!(
        call(&socket, "hello", Value::Null).is_err(),
        "nothing answers on the dead host's socket"
    );
    if let Some(helper) = helper {
        // The kernel ends the helper with its parent: no orphan is left holding a screen.
        let deadline = Instant::now() + Duration::from_secs(5);
        while alive(helper) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            !alive(helper),
            "the parser helper does not outlive the host"
        );
    }

    // A new host holds nothing of it, and starts nothing by itself.
    let mut second = sandbox.start_host();
    wait_for_socket(&sandbox);
    let listed = call(&socket, "term_list", Value::Null).unwrap();
    assert_eq!(
        listed.as_array().unwrap().len(),
        0,
        "no terminal is resurrected: {listed}"
    );
    std::thread::sleep(Duration::from_millis(500));
    assert!(!alive(pid));
    assert!(
        !read_all(&socket, &id, "x"),
        "the old terminal is unknown to the new host"
    );
    let _ = call(&socket, "shutdown", Value::Null);
    let _ = second.wait();
}
