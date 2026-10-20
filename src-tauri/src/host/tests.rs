//! F9: a host holds terminals and agents past any client, serves several at once, keeps a
//! bounded scrollback for one that attaches late, and is private to the user.
use super::{
    client::Remote,
    core::HostCore,
    protocol::{HostError, PROTOCOL},
    server::{self, BindError},
};
use crate::native::service_tests::World;
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

struct Host {
    core: Arc<HostCore>,
    socket: PathBuf,
    dirs: (PathBuf, PathBuf, PathBuf),
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Host {
    fn start(idle: Duration) -> Self {
        let dirs = World::directories();
        let socket = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("wyss-host-{}", uuid::Uuid::new_v4().simple()))
            .join("host.sock");
        let core = HostCore::new(Some(World::open(&dirs.0, &dirs.1, &dirs.2)));
        // Bash without the user's start-up files (the user's fish waits ten seconds for a
        // terminal query before it is ready, and prints a greeting).
        core.use_shell(dirs.2.join("bash"));
        let listener = server::bind(&socket).ok().expect("bind");
        let (c,) = (core.clone(),);
        let thread = Some(std::thread::spawn(move || server::run(listener, c, idle)));
        Self {
            core,
            socket,
            dirs,
            thread,
        }
    }
    fn client(&self) -> Remote {
        Remote::at(self.socket.clone())
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.client().call("shutdown", Value::Null);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        let _ = fs::remove_dir_all(self.socket.parent().unwrap());
        let _ = fs::remove_dir_all(self.dirs.0.parent().unwrap());
    }
}

const SIZE: Value = json!(null);
fn size() -> Value {
    json!({"rows": 24, "cols": 80})
}
fn text(value: &Value) -> String {
    String::from_utf8_lossy(
        &value["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b.as_u64().unwrap() as u8)
            .collect::<Vec<_>>(),
    )
    .into_owned()
}
/// Reads a terminal from the start until `needle` shows.
fn read_until(client: &Remote, id: &str, needle: &str) -> String {
    let mut seen = String::new();
    let mut from = 0u64;
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        let out = client
            .call(
                "term_read",
                json!({"terminal_id": id, "from": from, "wait_ms": 300}),
            )
            .unwrap();
        from = out["next"].as_u64().unwrap();
        seen.push_str(&text(&out));
        if seen.contains(needle) {
            return seen;
        }
    }
    panic!("never saw {needle:?}; got {seen:?}");
}
fn type_line(client: &Remote, id: &str, line: &str) {
    let bytes: Vec<u8> = format!("{line}\r").into_bytes();
    client
        .call("term_write", json!({"terminal_id": id, "data": bytes}))
        .unwrap();
}
const _: Value = SIZE;

#[test]
fn a_terminal_outlives_the_client_that_made_it_and_a_late_client_sees_what_was_printed() {
    let host = Host::start(Duration::from_secs(600));
    let first = host.client();
    let made = first
        .call("term_create", json!({"size": size(), "title": "Work"}))
        .unwrap();
    let id = made["terminal_id"].as_str().unwrap().to_owned();
    type_line(&first, &id, "echo marker-$((20+22))");
    read_until(&first, &id, "marker-42");
    drop(first);

    // A second client, later, from nothing: it lists the terminal with its title and reads the
    // scrollback from the start.
    let second = host.client();
    let list = second.call("term_list", Value::Null).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["title"], "Work");
    assert_eq!(list[0]["terminal"]["state"], "running");
    assert!(read_until(&second, &id, "marker-42").contains("marker-42"));

    // Closing it removes it from the host for everyone.
    second
        .call("term_close", json!({"terminal_id": id}))
        .unwrap();
    assert!(
        second
            .call("term_list", Value::Null)
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    // What the service remembers of it is that it was closed.
    let status = second
        .call("term_status", json!({"terminal_id": id}))
        .unwrap();
    assert_eq!(status["state"], "closed");
}

#[test]
fn an_agent_started_through_the_host_is_listed_with_its_tag_and_survives_its_client() {
    let host = Host::start(Duration::from_secs(600));
    let client = host.client();
    let home = host.dirs.1.to_string_lossy().into_owned();
    let reply = client
        .call(
            "native_start",
            json!({"agent_id": "claude_code", "project": home, "profile_id": null, "size": size()}),
        )
        .unwrap();
    let terminal = reply["terminal"]["terminal_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let session = reply["session"]["id"].as_str().unwrap().to_owned();
    read_until(&client, &terminal, "SESSION=");
    drop(client);

    let later = host.client();
    let list = later.call("term_list", Value::Null).unwrap();
    assert_eq!(list[0]["native"]["session_id"], json!(session));
    assert_eq!(list[0]["native"]["agent_name"], "Claude Code");
    assert_eq!(list[0]["title"], "Claude Code · home");
    let sessions = later
        .call("native_sessions", json!({"sample": true}))
        .unwrap();
    assert_eq!(sessions[0]["state"], "running");
    assert!(sessions[0]["usage"]["rss_bytes"].as_u64().unwrap() > 0);
    later.call("native_stop", json!({"id": session})).unwrap();
    let after = later
        .call("native_sessions", json!({"sample": false}))
        .unwrap();
    assert_eq!(after[0]["state"], "stopped");
    assert!(
        later
            .call("term_list", Value::Null)
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    let _ = host.core.idle_for();
}

#[test]
fn a_long_read_waits_for_new_output_and_a_short_one_does_not() {
    let host = Host::start(Duration::from_secs(600));
    let client = host.client();
    let id = client.call("term_create", json!({"size": size()})).unwrap()["terminal_id"]
        .as_str()
        .unwrap()
        .to_owned();
    // Let the start-up noise settle, then find where the output ends.
    read_until(&client, &id, "");
    std::thread::sleep(Duration::from_millis(800));
    let mut end = 0;
    loop {
        let out = client
            .call(
                "term_read",
                json!({"terminal_id": id, "from": end, "wait_ms": 0}),
            )
            .unwrap();
        let next = out["next"].as_u64().unwrap();
        if next == end {
            break;
        }
        end = next;
    }
    let started = Instant::now();
    let quiet = client
        .call(
            "term_read",
            json!({"terminal_id": id, "from": end, "wait_ms": 0}),
        )
        .unwrap();
    assert!(text(&quiet).is_empty() && started.elapsed() < Duration::from_millis(200));
    let writer = host.client();
    let id2 = id.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        type_line(&writer, &id2, "echo woke");
    });
    let long = client
        .call(
            "term_read",
            json!({"terminal_id": id, "from": end, "wait_ms": 2000}),
        )
        .unwrap();
    assert!(
        !text(&long).is_empty(),
        "the long read returned with the new output"
    );
    assert!(started.elapsed() < Duration::from_millis(1900));
}

#[test]
fn the_socket_is_private_a_second_host_is_refused_and_a_stale_socket_is_replaced() {
    let host = Host::start(Duration::from_secs(600));
    let dir = host.socket.parent().unwrap();
    assert_eq!(
        fs::metadata(dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&host.socket).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(matches!(
        server::bind(&host.socket),
        Err(BindError::AlreadyRunning)
    ));
    // A file that is not a socket is never replaced.
    let other = dir.join("other.sock");
    fs::write(&other, b"x").unwrap();
    assert!(matches!(server::bind(&other), Err(BindError::Unsafe)));
    // A socket nobody answers on is a leftover.
    let stale = dir.join("stale.sock");
    drop(std::os::unix::net::UnixListener::bind(&stale).unwrap());
    assert!(server::bind(&stale).is_ok());
}

#[test]
fn it_says_what_it_is_and_refuses_what_it_does_not_know() {
    let host = Host::start(Duration::from_secs(600));
    let client = host.client();
    let hello = client.call("hello", Value::Null).unwrap();
    assert_eq!(hello["protocol"], json!(PROTOCOL));
    // It names what it has beyond protocol 1, so a newer window can tell an older host.
    assert_eq!(hello["features"], json!(["profile_effort"]));
    assert_eq!(
        client.call("exec", json!({"cmd": "id"})).unwrap_err(),
        HostError::new("unknown_op")
    );
    assert_eq!(
        client
            .call("term_create", json!({"size": size(), "shell": "/bin/sh"}))
            .unwrap_err()
            .code,
        "request_invalid"
    );
    assert_eq!(
        client
            .call("term_write", json!({"terminal_id": "nope", "data": [1]}))
            .unwrap_err()
            .code,
        "request_invalid"
    );
    assert_eq!(
        client
            .call("native_remember_project", json!({"path": "relative"}))
            .unwrap_err()
            .code,
        "project_invalid"
    );
}

#[test]
fn a_host_with_nothing_to_hold_exits_by_itself_and_one_that_holds_a_terminal_does_not() {
    let host = Host::start(Duration::from_millis(400));
    let client = host.client();
    let id = client.call("term_create", json!({"size": size()})).unwrap()["terminal_id"]
        .as_str()
        .unwrap()
        .to_owned();
    std::thread::sleep(Duration::from_millis(1200));
    assert!(
        client.call("hello", Value::Null).is_ok(),
        "a terminal keeps it alive"
    );
    client
        .call("term_close", json!({"terminal_id": id}))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && client.call("hello", Value::Null).is_ok() {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(client.call("hello", Value::Null).is_err(), "idle, it left");
}

#[test]
fn profiles_travel_as_text_through_the_host() {
    let host = Host::start(Duration::from_secs(600));
    let client = host.client();
    let created = client
        .call(
            "profile_create",
            json!({"name": "Shared", "applies_to": ["codex"], "extends": null, "model": null,
                   "permission": null, "hooks": null, "skills_off": [], "notes": ""}),
        )
        .unwrap();
    let id = created["id"].as_str().unwrap();
    let exported = client.call("profile_export", json!({"id": id})).unwrap();
    let again = client
        .call("profile_import", json!({"text": exported["text"]}))
        .unwrap();
    assert_ne!(again["id"], created["id"]);
    let all = client.call("profiles", Value::Null).unwrap();
    assert!(
        all.as_array()
            .unwrap()
            .iter()
            .any(|p| p["body"]["name"] == "Shared (2)")
    );
    let _ = Path::new("/");
}

#[test]
fn a_handoff_brief_is_prepared_from_a_running_session_and_starts_nothing() {
    let host = Host::start(Duration::from_secs(600));
    let client = host.client();
    let home = host.dirs.1.to_string_lossy().into_owned();
    let reply = client
        .call(
            "native_start",
            json!({"agent_id": "claude_code", "project": home, "profile_id": null, "size": size(), "brief": null}),
        )
        .unwrap();
    let terminal = reply["terminal"]["terminal_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let session = reply["session"]["id"].as_str().unwrap().to_owned();
    read_until(&client, &terminal, "SESSION=");
    let before = client
        .call("native_sessions", json!({"sample": false}))
        .unwrap()
        .as_array()
        .unwrap()
        .len();
    let prepared = client
        .call("handoff_prepare", json!({"id": session}))
        .unwrap();
    let text = prepared["text"].as_str().unwrap();
    assert!(text.starts_with("# Handoff from Claude Code"));
    assert!(
        text.contains("FAKE-CLAUDE"),
        "the end of the screen is in it: {text}"
    );
    assert_eq!(prepared["screen_included"], true);
    assert_eq!(
        client
            .call("native_sessions", json!({"sample": false}))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        before
    );
    assert_eq!(
        client
            .call("handoff_prepare", json!({"id": "f".repeat(32)}))
            .unwrap_err()
            .code,
        "session_unknown"
    );
}

/// F11a against the installed agents: Claude Code runs in a folder with harmless test files,
/// its handoff brief is prepared (redacted, nothing started), a bad brief is refused and
/// leaves nothing behind, and the reviewed brief starts Codex as ONE argv word. No model is
/// called: both stop at their own first screen. By hand:
/// `cargo test -- --ignored real_handoff --nocapture`.
#[test]
#[ignore = "starts the installed Claude Code and Codex"]
fn real_handoff_between_two_installed_agents() {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!(
            "wyss-real-handoff-{}",
            uuid::Uuid::new_v4().simple()
        ));
    let (state, project) = (root.join("state"), root.join("project"));
    fs::create_dir_all(&project).unwrap();
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(args)
                .current_dir(&project)
                .output()
                .unwrap()
                .status
                .success()
        );
    };
    git(&["init", "-q"]);
    fs::write(project.join("notes.txt"), "one\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "first"]);
    // Uncommitted work, with a fictitious key where a brief could leak one.
    fs::write(project.join("notes.txt"), "one\ntwo\n").unwrap();
    let fake = concat!("sk-ant-", "api03-HANDOFFTESTFICTITIOUS0123456789");
    fs::write(project.join(format!("{fake}.txt")), "x").unwrap();
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let service =
        crate::native::service::NativeService::open(state, home, std::env::var_os("PATH")).unwrap();
    service.remember_chosen(project.to_string_lossy().into_owned());
    let core = HostCore::new(Some(service));
    let dir = root.join("sock");
    let socket = dir.join("host.sock");
    let listener = server::bind(&socket).ok().expect("bind");
    let c = core.clone();
    let thread = std::thread::spawn(move || server::run(listener, c, Duration::from_secs(600)));
    let client = Remote::at(socket);
    let folder = project.to_string_lossy().into_owned();
    let start = |agent: &str, brief: Value| {
        client.call(
            "native_start",
            json!({"agent_id": agent, "project": folder, "profile_id": null, "size": size(), "brief": brief}),
        )
    };

    let first = start("claude_code", Value::Null).unwrap();
    let (t1, s1) = (
        first["terminal"]["terminal_id"]
            .as_str()
            .unwrap()
            .to_owned(),
        first["session"]["id"].as_str().unwrap().to_owned(),
    );
    let mut screen = String::new();
    let deadline = Instant::now() + Duration::from_secs(40);
    let mut from = 0u64;
    while Instant::now() < deadline && !screen.contains("Accessing") {
        let out = client
            .call(
                "term_read",
                json!({"terminal_id": t1, "from": from, "wait_ms": 300}),
            )
            .unwrap();
        from = out["next"].as_u64().unwrap();
        screen.push_str(&text(&out));
    }
    assert!(
        screen.contains("Accessing"),
        "Claude Code's own screen: {screen:.200}"
    );

    // Prepare: redacted, from the real folder, and it starts nothing.
    let sessions = || {
        client
            .call("native_sessions", json!({"sample": false}))
            .unwrap()
            .as_array()
            .unwrap()
            .len()
    };
    let before = sessions();
    let prepared = client.call("handoff_prepare", json!({"id": s1})).unwrap();
    let brief = prepared["text"].as_str().unwrap().to_owned();
    assert!(brief.starts_with("# Handoff from Claude Code"));
    assert!(brief.contains(&folder) && brief.contains("notes.txt"));
    assert!(
        !brief.contains("HANDOFFTESTFICTITIOUS"),
        "a key leaked: {brief}"
    );
    assert_eq!(sessions(), before, "preparing started something");

    // Cancellation = never calling start; failure = a brief that cannot be one word.
    let bad = start("codex", json!("# x\0y")).unwrap_err();
    println!("bad brief refused with: {}", bad.code);
    assert_eq!(sessions(), before, "a refused brief left a session behind");

    // The reviewed brief starts the second, real agent.
    let edited = brief.replace("(Write it here before starting.)", "Say hello and stop.");
    let second = start("codex", json!(edited)).unwrap();
    let t2 = second["terminal"]["terminal_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let s2 = second["session"]["id"].as_str().unwrap().to_owned();
    let pid = second["terminal"]["observed_pid"].as_u64().unwrap();
    // The shell execs the agent a moment after the terminal exists: wait for its own argv.
    let mut cmdline = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        cmdline = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        if String::from_utf8_lossy(&cmdline).contains("# Handoff from") {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let words: Vec<&[u8]> = cmdline
        .split(|b| *b == 0)
        .filter(|w| !w.is_empty())
        .collect();
    let carried = words
        .iter()
        .filter(|w| String::from_utf8_lossy(w).starts_with("# Handoff from Claude Code"))
        .count();
    assert_eq!(
        carried,
        1,
        "the brief is one argv word of the new agent: {:.200}",
        String::from_utf8_lossy(&cmdline).replace('\0', " | ")
    );
    assert!(String::from_utf8_lossy(&cmdline).contains("Say hello and stop."));
    let mut seen = String::new();
    let mut from = 0u64;
    let deadline = Instant::now() + Duration::from_secs(40);
    while Instant::now() < deadline
        && !(seen.contains("Folder") || seen.contains("Background server"))
    {
        let out = client
            .call(
                "term_read",
                json!({"terminal_id": t2, "from": from, "wait_ms": 300}),
            )
            .unwrap();
        from = out["next"].as_u64().unwrap();
        seen.push_str(&text(&out));
    }
    assert!(
        seen.contains("Folder") || seen.contains("Background server"),
        "Codex's own screen: {seen:.200}"
    );
    println!(
        "handoff ok: Codex pid {pid} carries the brief ({} bytes)",
        edited.len()
    );
    client.call("native_stop", json!({"id": s2})).unwrap();
    client.call("native_stop", json!({"id": s1})).unwrap();
    let _ = client.call("shutdown", Value::Null);
    let _ = thread.join();
    let _ = fs::remove_dir_all(root);
}
