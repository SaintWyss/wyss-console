//! a host holds terminals and agents past any client, serves several at once, keeps a
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
    path::PathBuf,
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


/// The parser status of one terminal, as `term_list` reports it.
fn parser_of(client: &Remote, id: &str) -> Value {
    let list = client.call("term_list", Value::Null).unwrap();
    list.as_array()
        .and_then(|all| all.iter().find(|t| t["terminal"]["terminal_id"] == id))
        .map(|t| t["parser"].clone())
        .unwrap_or(Value::Null)
}

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
    assert_eq!(
        hello["features"],
        json!([
            "profile_effort",
            "native_profile_plan_token",
            "screen_snapshot",
            "native_arsenal_v1",
            "native_arsenal_codex_v1",
            "native_arsenal_launch_instructions_v1",
            "native_permission_bypass_v1",
            "native_template_edits_v1"
        ])
    );
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
    // Operations this app never calls are refused.
    for op in [
        "delegate_start",
        "handoff_prepare",
        "host_lingering",
        "native_launcher_extended",
        "native_remember_project",
        "term_parser",
    ] {
        assert_eq!(
            client.call(op, Value::Null).unwrap_err(),
            HostError::new("unknown_op"),
            "{op}"
        );
    }
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
fn the_host_has_no_operation_that_lists_or_changes_a_profile() {
    let host = Host::start(Duration::from_secs(600));
    let client = host.client();
    for (op, params) in [
        ("profiles", Value::Null),
        ("profiles_extended", Value::Null),
        (
            "profile_create",
            json!({"name": "Shared", "applies_to": ["codex"]}),
        ),
        ("profile_save", json!({"id": "a".repeat(32), "body": {}})),
        ("profile_delete", json!({"id": "a".repeat(32)})),
        ("profile_duplicate", json!({"id": "template-review"})),
        (
            "profile_restore",
            json!({"id": "a".repeat(32), "version": 1}),
        ),
        ("profile_export", json!({"id": "template-review"})),
        ("profile_import", json!({"text": "{}"})),
        (
            "native_project_default",
            json!({"project": "/", "profile_id": "template-full-access"}),
        ),
    ] {
        assert_eq!(
            client.call(op, params).unwrap_err().code,
            "unknown_op",
            "{op}"
        );
    }
    // The templates a permission is sent as are still resolved for a preview.
    for id in [
        "template-strict",
        "template-review",
        "template-edits",
        "template-full-access",
    ] {
        let preview = client
            .call(
                "native_preview",
                json!({"agent_id": "claude_code", "profile_id": id}),
            )
            .unwrap();
        assert_eq!(preview["can_start"], true, "{id}");
        assert!(preview["plan_token"].is_string(), "{id}");
    }
}


/// Voluntary context switches of one thread of this process: every time it went to sleep and was
/// woken. An idle thread that polls shows its poll rate here; one that waits shows almost none.
fn wakeups(tid: &str) -> Option<u64> {
    fs::read_to_string(format!("/proc/self/task/{tid}/status"))
        .ok()?
        .lines()
        .find_map(|l| {
            l.strip_prefix("voluntary_ctxt_switches:")?
                .trim()
                .parse()
                .ok()
        })
}
fn threads_named(prefix: &str) -> Vec<String> {
    fs::read_dir("/proc/self/task")
        .unwrap()
        .flatten()
        .filter(|t| {
            fs::read_to_string(t.path().join("comm"))
                .is_ok_and(|name| name.trim_end().starts_with(prefix))
        })
        .map(|t| t.file_name().to_string_lossy().into_owned())
        .collect()
}

/// An idle host waits for socket connections and terminal output without repeatedly polling the PTY.
#[test]
fn a_host_with_an_idle_terminal_waits_instead_of_polling() {
    let _one_at_a_time = CAP_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    let dirs = World::directories();
    let socket = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("wyss-host-{}", uuid::Uuid::new_v4().simple()))
        .join("host.sock");
    let core = HostCore::new(Some(World::open(&dirs.0, &dirs.1, &dirs.2)));
    core.use_shell(dirs.2.join("bash"));
    let listener = server::bind(&socket).ok().expect("bind");
    let (tx, rx) = std::sync::mpsc::channel();
    let c = core.clone();
    let thread = std::thread::spawn(move || {
        tx.send(nix::unistd::gettid().as_raw().to_string()).unwrap();
        server::run(listener, c, Duration::from_secs(600));
    });
    let accept = rx.recv().unwrap();
    let client = Remote::at(socket.clone());
    let id = client.call("term_create", json!({"size": size()})).unwrap()["terminal_id"]
        .as_str()
        .unwrap()
        .to_owned();
    // The shell has started and printed what it prints; from here on nothing happens.
    read_until(&client, &id, "$");
    std::thread::sleep(Duration::from_secs(2));
    // Only this terminal's own threads (other tests of this process run in parallel).
    let pumps = threads_named(&format!("wyss-pump-{}", &id[..5]));
    let ptys = threads_named(&format!("wyss-pty-{}", &id[..6]));
    let links = threads_named(&format!("wyss-sl-{}", &id[..7]));
    assert!(!pumps.is_empty() && !ptys.is_empty());
    let all = |tids: &[String]| tids.iter().filter_map(|t| wakeups(t)).sum::<u64>();
    let before = (wakeups(&accept).unwrap(), all(&pumps), all(&ptys));
    let link_before = all(&links);
    let window = 4.0;
    std::thread::sleep(Duration::from_secs(4));
    let after = (wakeups(&accept).unwrap(), all(&pumps), all(&ptys));
    let rate = |a: u64, b: u64| (b - a) as f64 / window;
    let (accept_rate, pump_rate, pty_rate) = (
        rate(before.0, after.0),
        rate(before.1, after.1),
        rate(before.2, after.2),
    );
    let link_rate = (all(&links) - link_before) as f64 / window;
    println!(
        "idle wake-ups per second: accept {accept_rate:.1}, pump {pump_rate:.1}, pty {pty_rate:.1}, parser link {link_rate:.1} ({} thread)",
        links.len()
    );
    let _ = client.call("term_close", json!({"terminal_id": id}));
    let _ = client.call("shutdown", Value::Null);
    let _ = thread.join();
    let _ = fs::remove_dir_all(socket.parent().unwrap());
    let _ = fs::remove_dir_all(dirs.0.parent().unwrap());
    assert!(accept_rate < 8.0, "the accept loop polls: {accept_rate}/s");
    assert!(pump_rate < 4.0, "the pump polls: {pump_rate}/s");
    // The PTY worker still looks at its child a few times a second, to see it exit.
    assert!(pty_rate < 12.0, "the PTY worker polls: {pty_rate}/s");
    // The parser link has nothing to wake for while the terminal is idle.
    assert!(link_rate < 0.5, "the parser link polls: {link_rate}/s");
}

// ---- The screen a terminal keeps in a parser helper ---------------------------------------

mod screen {
    use super::*;
    use wyss_console::screen::{Limits, Terminal, b64};

    fn helper_there(host: &Host) -> bool {
        let id = host
            .client()
            .call("term_create", json!({"size": size(), "title": "probe"}))
            .unwrap();
        let id = id["terminal_id"].as_str().unwrap().to_owned();
        let parser = parser_of(&host.client(), &id);
        let there = parser["health"] != "unavailable" || parser["reason"] != "helper_missing";
        host.client()
            .call("term_close", json!({"terminal_id": id}))
            .unwrap();
        there
    }

    fn screen_of(client: &Remote, id: &str) -> Value {
        client
            .call("term_screen", json!({"terminal_id": id}))
            .unwrap()
    }
    fn rows_of(answer: &Value, rows: usize, cols: usize) -> Vec<String> {
        let bytes = b64::decode(answer["repaint"].as_str().unwrap()).unwrap();
        let mut t = Terminal::new(rows, cols, Limits::default());
        t.feed(&bytes);
        t.text()
    }
    fn wait_parser(client: &Remote, id: &str, health: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let p = parser_of(client, id);
            if p["health"] == health || Instant::now() > deadline {
                return p;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn a_late_client_receives_the_screen_a_repaint_and_where_in_the_output_it_is() {
        let host = Host::start(Duration::from_secs(600));
        if !helper_there(&host) {
            eprintln!("NOT_RUN: no wyss-screen next to the test binary");
            return;
        }
        let first = host.client();
        let made = first
            .call("term_create", json!({"size": size(), "title": "Work"}))
            .unwrap();
        let id = made["terminal_id"].as_str().unwrap().to_owned();
        type_line(
            &first,
            &id,
            "printf '\\033[2J\\033[Hscreen-marker \\033[1;31mred\\033[0m\\n'",
        );
        read_until(&first, &id, "red\u{1b}[0m");
        drop(first);

        let late = host.client();
        let answer = screen_of(&late, &id);
        assert_eq!(answer["available"], true, "{answer}");
        assert_eq!(answer["parser"]["health"], "healthy");
        assert_eq!(answer["parser"]["fidelity"], "exact");
        assert_eq!(answer["parser"]["sandbox"]["layers"], 15);
        assert!(
            rows_of(&answer, 24, 80)[0].starts_with("screen-marker red"),
            "{:?}",
            rows_of(&answer, 24, 80)
        );
        // From `at`, the bytes that follow are exactly the ones the repaint does not contain.
        let at = answer["at"].as_u64().unwrap();
        type_line(&late, &id, "echo after-attach-77");
        let mut from = at;
        let mut seen = String::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !seen.contains("after-attach-77") && Instant::now() < deadline {
            let out = late
                .call(
                    "term_read",
                    json!({"terminal_id": id, "from": from, "wait_ms": 300}),
                )
                .unwrap();
            from = out["next"].as_u64().unwrap();
            seen.push_str(&text(&out));
        }
        assert!(seen.contains("after-attach-77"));
        assert!(
            !seen.contains("screen-marker"),
            "the repaint's content is not delivered a second time"
        );
    }

    #[test]
    fn killing_the_parser_helper_does_not_touch_the_terminal_and_the_screen_comes_back() {
        let host = Host::start(Duration::from_secs(600));
        if !helper_there(&host) {
            eprintln!("NOT_RUN: no wyss-screen next to the test binary");
            return;
        }
        let client = host.client();
        let made = client
            .call("term_create", json!({"size": size(), "title": "Work"}))
            .unwrap();
        let id = made["terminal_id"].as_str().unwrap().to_owned();
        let shell_pid = made["observed_pid"].as_u64();
        type_line(&client, &id, "echo first-marker-1");
        read_until(&client, &id, "first-marker-1");
        let before = screen_of(&client, &id);
        assert_eq!(before["available"], true);
        // The helper this host started, ended the way a memory killer would end it.
        let helper_pid = before["parser"]["helper_pid"]
            .as_i64()
            .expect("the link reports its helper") as i32;
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(helper_pid),
            nix::sys::signal::Signal::SIGKILL,
        )
        .unwrap();
        // The shell never noticed.
        type_line(&client, &id, "echo second-marker-2");
        read_until(&client, &id, "second-marker-2");
        let info = client
            .call("term_status", json!({"terminal_id": id}))
            .unwrap();
        assert_eq!(info["state"], "running");
        assert_eq!(info["observed_pid"].as_u64(), shell_pid);
        // And the screen is rebuilt from the ring, with both lines.
        let after = screen_of(&client, &id);
        assert_eq!(after["available"], true, "{after}");
        let rows = rows_of(&after, 24, 80).join("\n");
        assert!(
            rows.contains("first-marker-1") && rows.contains("second-marker-2"),
            "{rows}"
        );
        assert!(after["parser"]["restarts"].as_u64().unwrap() >= 1);
        assert_eq!(after["parser"]["fidelity"], "exact");
        let _ = wait_parser(&client, &id, "healthy");
    }

    #[test]
    fn a_burst_of_resizes_while_output_flows_leaves_the_parser_at_the_ptys_final_size() {
        let host = Host::start(Duration::from_secs(600));
        if !helper_there(&host) {
            eprintln!("NOT_RUN: no wyss-screen next to the test binary");
            return;
        }
        let client = host.client();
        let id = client.call("term_create", json!({"size": size()})).unwrap()["terminal_id"]
            .as_str()
            .unwrap()
            .to_owned();
        type_line(
            &client,
            &id,
            "for i in $(seq 1 300); do echo line-$i; done; echo flood-done",
        );
        let sizes = [
            (24, 80),
            (30, 100),
            (10, 40),
            (50, 132),
            (24, 80),
            (40, 120),
            (12, 60),
        ];
        for (rows, cols) in sizes.iter().cycle().take(40) {
            client
                .call(
                    "term_resize",
                    json!({"terminal_id": id, "size": {"rows": rows, "cols": cols}}),
                )
                .unwrap();
        }
        read_until(&client, &id, "flood-done");
        let info = client
            .call("term_status", json!({"terminal_id": id}))
            .unwrap();
        let answer = screen_of(&client, &id);
        assert_eq!(answer["available"], true, "{answer}");
        assert_eq!(
            answer["size"], info["size"],
            "the parser follows the one size the PTY has"
        );
        assert_eq!(answer["parser"]["health"], "healthy");
    }

    #[test]
    fn attaching_again_and_again_creates_nothing_and_changes_no_process() {
        let host = Host::start(Duration::from_secs(600));
        if !helper_there(&host) {
            eprintln!("NOT_RUN: no wyss-screen next to the test binary");
            return;
        }
        let client = host.client();
        let made = client
            .call("term_create", json!({"size": size(), "title": "Work"}))
            .unwrap();
        let id = made["terminal_id"].as_str().unwrap().to_owned();
        let pid = made["observed_pid"].as_u64();
        type_line(&client, &id, "echo once-only-9");
        read_until(&client, &id, "\u{1b}[?2004l\ronce-only-9\r\n");
        for _ in 0..8 {
            let answer = screen_of(&host.client(), &id);
            assert_eq!(answer["available"], true);
        }
        let list = client.call("term_list", Value::Null).unwrap();
        assert_eq!(
            list.as_array().unwrap().len(),
            1,
            "no second terminal was made"
        );
        assert_eq!(
            client
                .call("term_status", json!({"terminal_id": id}))
                .unwrap()["observed_pid"]
                .as_u64(),
            pid
        );
        // Nothing was typed to the program by attaching: the command ran exactly once.
        let ring = read_until(&client, &id, "\u{1b}[?2004l\ronce-only-9\r\n");
        // The echo of the typed line shows more than once (the line discipline and the shell's
        // own redisplay); what the command *printed* is the line that follows the end of the
        // bracketed-paste mode and a carriage return, and there is exactly one.
        assert_eq!(
            ring.matches("\u{1b}[?2004l\ronce-only-9\r\n").count(),
            1,
            "the command ran once: {ring:?}"
        );
    }

    #[test]
    fn an_exited_terminal_keeps_its_last_screen_and_says_it_exited() {
        let host = Host::start(Duration::from_secs(600));
        if !helper_there(&host) {
            eprintln!("NOT_RUN: no wyss-screen next to the test binary");
            return;
        }
        let client = host.client();
        let id = client.call("term_create", json!({"size": size()})).unwrap()["terminal_id"]
            .as_str()
            .unwrap()
            .to_owned();
        type_line(&client, &id, "echo last-words-3; exit 7");
        read_until(&client, &id, "last-words-3");
        let deadline = Instant::now() + Duration::from_secs(10);
        let state = loop {
            let info = client
                .call("term_status", json!({"terminal_id": id}))
                .unwrap();
            if info["state"] == "exited" || Instant::now() > deadline {
                break info;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        assert_eq!(state["state"], "exited");
        assert_eq!(state["exit"]["code"], 7);
        let answer = screen_of(&client, &id);
        assert_eq!(answer["available"], true, "{answer}");
        assert!(rows_of(&answer, 24, 80).join("\n").contains("last-words-3"));
        // Nothing relaunched it.
        assert_eq!(
            client
                .call("term_status", json!({"terminal_id": id}))
                .unwrap()["state"],
            "exited"
        );
    }

    #[test]
    fn input_still_reaches_the_program_while_nobody_is_reading_its_flood() {
        let host = Host::start(Duration::from_secs(600));
        let client = host.client();
        let id = client.call("term_create", json!({"size": size()})).unwrap()["terminal_id"]
            .as_str()
            .unwrap()
            .to_owned();
        // A flood of about 3 MiB that nobody reads (the ring keeps its bound), then a command.
        type_line(
            &client,
            &id,
            "head -c 3000000 /dev/zero | tr '\\0' 'x'; echo",
        );
        std::thread::sleep(Duration::from_millis(500));
        type_line(&client, &id, "echo after-the-flood-8");
        let late = host.client();
        // A slow reader that starts at the oldest byte it can still have is told what it missed.
        let mut from = 0u64;
        let mut truncated = false;
        let mut seen = String::new();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !seen.contains("after-the-flood-8") && Instant::now() < deadline {
            let out = late
                .call(
                    "term_read",
                    json!({"terminal_id": id, "from": from, "wait_ms": 300}),
                )
                .unwrap();
            truncated |= out["truncated"] == true;
            from = out["next"].as_u64().unwrap();
            seen.push_str(&text(&out));
        }
        assert!(
            seen.contains("after-the-flood-8"),
            "the command typed during the flood ran"
        );
        assert!(truncated, "and the reader was told that it missed output");
    }

    #[test]
    fn without_a_helper_a_terminal_still_works_and_the_window_is_told_to_replay() {
        let host = Host::start(Duration::from_secs(600));
        host.core.use_screen_spawner(None);
        let client = host.client();
        let made = client
            .call("term_create", json!({"size": size(), "title": "Work"}))
            .unwrap();
        let id = made["terminal_id"].as_str().unwrap().to_owned();
        type_line(&client, &id, "echo replay-marker-5");
        read_until(&client, &id, "replay-marker-5");
        let answer = screen_of(&client, &id);
        assert_eq!(answer["available"], false);
        assert_eq!(answer["reason"], "helper_missing");
        assert_eq!(answer["parser"]["health"], "unavailable");
        let list = client.call("term_list", Value::Null).unwrap();
        assert_eq!(list[0]["parser"]["fidelity"], "unavailable");
    }

}

// ---- Screen measurements (run in release) -------

mod perf {
    use super::*;

    fn pct(v: &mut [f64], q: f64) -> f64 {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[((v.len() as f64 * q) as usize).min(v.len() - 1)]
    }
    fn line(what: &str, unit: &str, samples: &mut [f64], extra: &str) {
        let raw: Vec<f64> = samples
            .iter()
            .map(|v| (v * 1000.0).round() / 1000.0)
            .collect();
        println!(
            "\n{{\"measure\":\"{what}\",\"unit\":\"{unit}\",\"n\":{},\"p50\":{:.3},\"p95\":{:.3},{extra}\"raw\":{raw:?}}}",
            samples.len(),
            pct(samples, 0.5),
            pct(samples, 0.95)
        );
    }
    fn kib(pid: i64, key: &str) -> f64 {
        fs::read_to_string(format!("/proc/{pid}/status"))
            .unwrap()
            .lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap()
            .parse()
            .unwrap()
    }

    #[test]
    #[ignore = "measurement"]
    fn attach_latency_and_sixteen_terminals() {
        let host = Host::start(Duration::from_secs(600));
        let client = host.client();
        let make = || {
            let made = client.call("term_create", json!({"size": size()})).unwrap();
            made["terminal_id"].as_str().unwrap().to_owned()
        };
        // One busy terminal: a screenful and a long history.
        let busy = make();
        type_line(
            &client,
            &busy,
            "for i in $(seq 1 1500); do echo \"history line $i with some text to fill a row of the screen\"; done; echo ready-now",
        );
        read_until(&client, &busy, "ready-now");
        let mut ms = Vec::new();
        let mut bytes = 0usize;
        for _ in 0..40 {
            let started = Instant::now();
            let answer = client
                .call("term_screen", json!({"terminal_id": busy}))
                .unwrap();
            ms.push(started.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(answer["available"], true, "{answer}");
            bytes = answer["repaint"].as_str().unwrap().len() * 3 / 4;
        }
        line(
            "term_screen_latency_24x80_history_500",
            "ms",
            &mut ms,
            &format!("\"repaint_bytes\":{bytes},"),
        );

        // Sixteen terminals, each with a parser helper: what they cost at rest.
        let mut ids = vec![busy];
        while ids.len() < 16 {
            ids.push(make());
        }
        for id in &ids {
            type_line(&client, id, "echo sixteen-$((6*7))");
            read_until(&client, id, "sixteen-42");
        }
        std::thread::sleep(Duration::from_secs(2));
        let mut rss = Vec::new();
        let mut peak = Vec::new();
        let (mut threads, mut fds) = (0usize, 0usize);
        for id in &ids {
            let p = parser_of(&client, id);
            assert_eq!(p["health"], "healthy", "{p}");
            let pid = p["helper_pid"].as_i64().unwrap();
            rss.push(kib(pid, "VmRSS:") / 1024.0);
            peak.push(kib(pid, "VmHWM:") / 1024.0);
            threads += fs::read_dir(format!("/proc/{pid}/task")).unwrap().count();
            // Not readable for a non-dumpable helper: counted only where it can be read.
            fds += fs::read_dir(format!("/proc/{pid}/fd")).map_or(0, |d| d.count());
        }
        let total: f64 = rss.iter().sum();
        line(
            "helper_rss_per_terminal_16_terminals",
            "MiB",
            &mut rss,
            &format!(
                "\"sum_mib\":{total:.2},\"helper_threads\":{threads},\"helper_descriptors\":{fds},"
            ),
        );
        line(
            "helper_peak_rss_per_terminal_16_terminals",
            "MiB",
            &mut peak,
            "",
        );
        // The host's own threads per terminal (pump, PTY worker, link, its two pipe threads).
        let host_threads = fs::read_dir("/proc/self/task").unwrap().count();
        println!(
            "\n{{\"measure\":\"threads_in_the_host_process_with_16_terminals\",\"threads\":{host_threads}}}"
        );
        // The sixteenth place is needed below.
        let spare = ids.pop().unwrap();
        client
            .call("term_close", json!({"terminal_id": spare}))
            .unwrap();

        // Start of a terminal to a first screen the window could attach to.
        let mut first = Vec::new();
        for _ in 0..10 {
            let started = Instant::now();
            let id = make();
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                let a = client
                    .call("term_screen", json!({"terminal_id": id}))
                    .unwrap();
                if a["available"] == true {
                    break;
                }
                assert!(Instant::now() < deadline, "never attachable: {a}");
                std::thread::sleep(Duration::from_millis(5));
            }
            first.push(started.elapsed().as_secs_f64() * 1000.0);
            client
                .call("term_close", json!({"terminal_id": id}))
                .unwrap();
        }
        line(
            "terminal_create_to_first_attachable_screen",
            "ms",
            &mut first,
            "",
        );

        // Recovery: a helper killed, to a screen that is exact again.
        let mut recover = Vec::new();
        // Three: the restart budget is three in a minute, and a measurement must not use it up.
        for _ in 0..3 {
            let p = parser_of(&client, &ids[0]);
            let before = p["restarts"].as_u64().unwrap();
            let pid = p["helper_pid"].as_i64().unwrap() as i32;
            let started = Instant::now();
            nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            )
            .unwrap();
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                let p = parser_of(&client, &ids[0]);
                if p["health"] == "healthy"
                    && p["restarts"].as_u64().unwrap() > before
                    && p["fed"].as_u64().unwrap() > 0
                {
                    break;
                }
                assert!(Instant::now() < deadline, "never recovered: {p}");
                std::thread::sleep(Duration::from_millis(2));
            }
            recover.push(started.elapsed().as_secs_f64() * 1000.0);
            std::thread::sleep(Duration::from_secs(2));
        }
        line(
            "helper_killed_to_healthy_and_fed_again",
            "ms",
            &mut recover,
            "",
        );
    }
}

/// The deadline covers the entire request frame even when bytes keep arriving.
#[test]
fn a_client_that_drips_its_request_is_cut_off_at_the_frame_deadline() {
    use std::io::Write;
    let host = Host::start(Duration::from_secs(600));
    let mut stream = std::os::unix::net::UnixStream::connect(&host.socket).unwrap();
    let started = Instant::now();
    // A length prefix promising a frame that never completes, one byte per second.
    let _ = stream.write_all(&[0, 0, 0]);
    for _ in 0..12 {
        std::thread::sleep(Duration::from_secs(1));
        if stream.write_all(&[0]).is_err() {
            break;
        }
    }
    assert!(
        started.elapsed() < server::FRAME_DEADLINE + Duration::from_secs(3),
        "the host held the connection for {:?}",
        started.elapsed()
    );
}

/// Tests that count the threads or wake-ups of the whole process run one at a time.
static CAP_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// With many silent connections open, a new client is still answered and the host does not hold
/// more than its cap of connection threads.
#[test]
fn silent_connections_do_not_starve_a_new_client_nor_grow_without_bound() {
    let _one_at_a_time = CAP_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    let host = Host::start(Duration::from_secs(600));
    let silent: Vec<_> = (0..100)
        .map(|_| std::os::unix::net::UnixStream::connect(&host.socket).unwrap())
        .collect();
    std::thread::sleep(Duration::from_millis(300));
    let hello = host.client().call("hello", Value::Null).unwrap();
    assert_eq!(hello["protocol"], json!(PROTOCOL));
    assert!(threads_named("wyss-host-conn").len() <= server::MAX_CONNECTIONS + 2);
    drop(silent);
}

/// Long polls fill their share of the cap, never the whole of it: a new client is still answered.
#[test]
fn a_full_set_of_long_polls_still_leaves_room_for_short_calls() {
    let _one_at_a_time = CAP_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    let host = Host::start(Duration::from_secs(600));
    let id = host.client().call("term_create", json!({"size": size()})).unwrap()["terminal_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let pollers: Vec<_> = (0..server::MAX_CONNECTIONS)
        .map(|_| {
            let (client, id) = (host.client(), id.clone());
            std::thread::spawn(move || {
                let _ = client.call(
                    "term_read",
                    json!({"terminal_id": id, "from": 1u64 << 40, "wait_ms": 3000}),
                );
            })
        })
        .collect();
    std::thread::sleep(Duration::from_millis(600));
    let started = Instant::now();
    let hello = host.client().call("hello", Value::Null).unwrap();
    assert_eq!(hello["protocol"], json!(PROTOCOL));
    assert!(started.elapsed() < Duration::from_secs(2));
    for p in pollers {
        let _ = p.join();
    }
}
