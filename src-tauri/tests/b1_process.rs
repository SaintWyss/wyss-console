//! The constrained B1 candidate, observed from inside and from outside (docs 49–52, ADR 0058).
//!
//! Every denial is paired with a *positive control*: the same probe, run without confinement,
//! succeeds in the attack, so "denied" means something. Workers are the `wyss-b1-probe` binary and
//! `/usr/bin/sh`, started by the real launch plan, `bwrap` and shim; the secrets are synthetic
//! strings in files this test creates; the "authority" is a private `wyss-control` in a private
//! runtime directory. Nothing of the installed Console, its host, its authority, its credentials or
//! the user's home is named or touched, and no network packet leaves the machine.
//!
//! Where the machine cannot run a check (no user namespaces, Landlock older than ABI 6) the test
//! prints `NOT_RUN: <why>`, records it in `b1-not-run.txt` and passes without claiming anything (the
//! gate stays where it was); with `WYSS_B1_REQUIRE_RUN=1`, as in the acceptance run, it fails instead.
#![cfg(all(target_os = "linux", feature = "b1-fixture"))]

use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, PermissionsExt},
        net::UnixListener,
        process::ExitStatusExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use wyss_console::{
    b1::{
        boundary,
        confine::{self, layer},
        plan::{Plan, PlanError},
        workspace::{self, Workspace},
    },
    contracts::b1::{
        B1ErrorCode, GitOid, Sha256Hex,
        admission::{
            AdmittedSnapshot, BaseTree, SuppliedContent, SuppliedRecord, TaskContext, admit,
        },
        digest,
        path::B1RelPath,
        records::ExportOperation,
        source::{B1SourceSelectionV1, ChangeOrigin, EXECUTABLE_MODE, REGULAR_MODE},
    },
};

const SHIM: &str = env!("CARGO_BIN_EXE_wyss-b1-shim");
const PROBE: &str = env!("CARGO_BIN_EXE_wyss-b1-probe");
const CONTROL: &str = env!("CARGO_BIN_EXE_wyss-control");

const SIGSYS: i32 = 31;
const CANARY: &str = "SYNTHETIC-FORBIDDEN-SIGNER-CANARY-7f3a";
const ENV_CANARY: &str = "SYNTHETIC-ENV-CANARY-91c2";

// ---- the rig ---------------------------------------------------------------------------------

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Rig {
    root: PathBuf,
    shim: PathBuf,
    probe: PathBuf,
}

impl Rig {
    fn new(name: &str) -> Self {
        // Short, because unix socket paths are limited to 107 bytes; private, because the plan wants
        // it; under the system temporary directory, whose sticky bit the helper check understands.
        let root = std::env::temp_dir().join(format!(
            "b1p-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = name;
        let _ = fs::remove_dir_all(&root);
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let root = fs::canonicalize(&root).unwrap();
        // The build's own artifacts are whatever the build's umask made them; the launch plan
        // accepts only helpers nobody else can write, so they are copied into a private directory.
        let bin = root.join("bin");
        fs::DirBuilder::new().mode(0o700).create(&bin).unwrap();
        let copy = |from: &str, name: &str| {
            let to = bin.join(name);
            fs::copy(from, &to).unwrap();
            fs::set_permissions(&to, fs::Permissions::from_mode(0o755)).unwrap();
            to
        };
        Self {
            shim: copy(SHIM, "shim"),
            probe: copy(PROBE, "probe"),
            root,
        }
    }
    fn dir(&self, name: &str) -> PathBuf {
        let d = self.root.join(name);
        fs::DirBuilder::new()
            .mode(0o700)
            .recursive(true)
            .create(&d)
            .unwrap();
        fs::canonicalize(&d).unwrap()
    }
    /// A tree built from an admitted snapshot, as the real pipeline would.
    fn workspace(&self, files: &[(&str, &[u8], u32)]) -> Workspace {
        let (snapshot, context) = snapshot(files);
        workspace::materialize(&self.dir("tasks"), &snapshot, &context).expect("materialized")
    }
    fn plan(&self, ws: &Workspace) -> Plan {
        Plan::for_workspace(ws)
    }
    fn run(&self, plan: &Plan, worker: &str, args: &[&str]) -> Ran {
        self.run_with(plan, worker, args, &[])
    }
    /// `inherit`: descriptors this test leaves open across the exec on purpose (the leak control).
    fn run_with(&self, plan: &Plan, worker: &str, args: &[&str], inherit: &[i32]) -> Ran {
        let (read, write) = nix::unistd::pipe2(nix::fcntl::OFlag::O_CLOEXEC).unwrap();
        let write = fs::File::from(write);
        let args: Vec<_> = args.iter().map(std::ffi::OsString::from).collect();
        let mut launch = plan
            .launch(
                &self.shim,
                &[("probe", &self.probe)],
                worker,
                &args,
                Some(&write),
            )
            .expect("a plan");
        let _ = inherit;
        let child = launch.command.spawn().expect("bwrap starts");
        drop(write);
        drop(launch);
        let mut ran = finish(child, Duration::from_secs(30));
        let mut receipt = String::new();
        let _ = fs::File::from(read).read_to_string(&mut receipt);
        ran.receipt = serde_json::from_str(receipt.trim()).ok();
        ran
    }
    /// One probe action inside confinement.
    fn probe(&self, plan: &Plan, args: &[&str]) -> Probe {
        let mut all = vec![];
        all.extend_from_slice(args);
        Probe::from(self.run(plan, "/b1/bin/probe", &all))
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct Ran {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
    receipt: Option<Value>,
}

fn finish(mut child: Child, limit: Duration) -> Ran {
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let o = thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let e = thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        s
    });
    let end = Instant::now() + limit;
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if Instant::now() > end {
            // The launcher is this test's own child: ended through its handle.
            let _ = child.kill();
            break child.wait().unwrap();
        }
        thread::sleep(Duration::from_millis(10));
    };
    Ran {
        status,
        stdout: o.join().unwrap(),
        stderr: e.join().unwrap(),
        receipt: None,
    }
}

/// What a probe printed, and how it ended.
#[derive(Debug)]
struct Probe {
    value: Option<Value>,
    signal: Option<i32>,
    code: Option<i32>,
    stderr: String,
    receipt: Option<Value>,
}

impl From<Ran> for Probe {
    fn from(r: Ran) -> Self {
        Probe {
            value: r
                .stdout
                .lines()
                .last()
                .and_then(|l| serde_json::from_str(l).ok()),
            signal: r.status.signal(),
            code: r.status.code(),
            stderr: r.stderr,
            receipt: r.receipt,
        }
    }
}

impl Probe {
    fn ok(&self) -> bool {
        self.value.as_ref().is_some_and(|v| v["ok"] == true)
    }
    fn errno(&self) -> i64 {
        self.value
            .as_ref()
            .and_then(|v| v["errno"].as_i64())
            .unwrap_or(-99)
    }
    fn detail(&self) -> &Value {
        self.value.as_ref().map_or(&Value::Null, |v| &v["detail"])
    }
    fn killed_by_seccomp(&self) -> bool {
        // The sandbox's init reports a killed worker as 128 + the signal.
        self.signal == Some(SIGSYS) || self.code == Some(128 + SIGSYS)
    }
    fn denied(&self) -> bool {
        !self.ok() && self.value.is_some()
    }
}

/// The same probe with no confinement: the attack control.
fn outside(args: &[&str]) -> Probe {
    let child = Command::new(PROBE)
        .args(args)
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    Probe::from(finish(child, Duration::from_secs(10)))
}

/// A check this machine cannot run. It is printed, **appended to a file** (cargo hides the output of a
/// passing test) and, when `WYSS_B1_REQUIRE_RUN=1` (the acceptance run), it fails instead: a skipped
/// attack is never a green one there.
fn not_run(why: &str) {
    eprintln!("NOT_RUN: {why}");
    let dir = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let _ = fs::create_dir_all(&dir);
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("b1-not-run.txt"))
    {
        let _ = writeln!(f, "{why}");
    }
    assert!(
        std::env::var_os("WYSS_B1_REQUIRE_RUN").is_none(),
        "WYSS_B1_REQUIRE_RUN is set and this check could not run: {why}"
    );
}

/// `false` (after recording `NOT_RUN`) only when the *machine* lacks something: Landlock ABI 6 or user
/// namespaces, tried with a bare `bwrap` that does not involve the candidate. Once the machine is
/// shown capable, a launch that does not work is a regression of the candidate and **fails**.
fn machine_ok(rig: &Rig) -> bool {
    let abi = confine::landlock_abi();
    if abi < confine::MIN_LANDLOCK_ABI {
        not_run(&format!(
            "Landlock ABI {abi} < {}",
            confine::MIN_LANDLOCK_ABI
        ));
        return false;
    }
    let bare = Command::new("/usr/bin/bwrap")
        .env_clear()
        .args([
            "--unshare-user",
            "--unshare-pid",
            "--unshare-net",
            "--ro-bind",
            "/usr",
            "/usr",
            "--symlink",
            "usr/bin",
            "/bin",
            "--symlink",
            "usr/lib",
            "/lib",
            "--symlink",
            "usr/lib",
            "/lib64",
            "/usr/bin/true",
        ])
        .stdin(Stdio::null())
        .output();
    if !bare.as_ref().is_ok_and(|o| o.status.success()) {
        not_run(&format!(
            "this machine cannot make user namespaces with bwrap: {:?}",
            bare.map(|o| String::from_utf8_lossy(&o.stderr).trim().to_owned())
        ));
        return false;
    }
    // From here the machine can run the candidate: a failure below is the candidate's.
    let ws = rig.workspace(PROJECT);
    let plan = rig.plan(&ws);
    let p = rig.probe(&plan, &["who"]);
    assert!(
        p.value.is_some(),
        "the machine can run bwrap and Landlock ABI >= 6, but the candidate did not start a worker: \
         code={:?} signal={:?} stderr={}",
        p.code,
        p.signal,
        p.stderr.trim()
    );
    ws.remove();
    true
}

macro_rules! rig {
    ($name:expr) => {{
        let rig = Rig::new($name);
        if !machine_ok(&rig) {
            return;
        }
        rig
    }};
}

// ---- admitted snapshots ----------------------------------------------------------------------

const TASK: &str = "11111111-1111-4111-8111-111111111111";
const BASE: &str = "0123456789abcdef0123456789abcdef01234567";

fn typed<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

fn snapshot(files: &[(&str, &[u8], u32)]) -> (AdmittedSnapshot, TaskContext) {
    let mut sorted: Vec<_> = files.to_vec();
    sorted.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let overlay: Vec<Value> = sorted
        .iter()
        .map(|(path, bytes, mode)| {
            json!({
                "path": serde_json::to_value(B1RelPath::from_bytes(path.as_bytes()).unwrap()).unwrap(), "operation": "add",
                "origin": "untracked", "expected_base_oid": null, "rename_from": null,
                "content": {"kind": "regular", "content_sha256": digest::raw_sha256(bytes).as_str(),
                            "size": bytes.len(), "mode": mode},
                "reviewed_sha256": "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"
            })
        })
        .collect();
    let selection: B1SourceSelectionV1 = typed(json!({
        "schema_version": 1, "task_id": TASK,
        "selection_id": "22222222-2222-4222-8222-222222222222",
        "repo_id": "33333333-3333-4333-8333-333333333333",
        "base_commit_oid": BASE, "repo_object_format": "sha1", "admitted_overlay": overlay,
        "policy_id": "44444444-4444-4444-8444-444444444444",
        "policy_sha256": "abababababababababababababababababababababababababababababababab",
        "issued_at": "2026-10-16T10:00:00Z"
    }));
    let base = BaseTree::new(typed::<GitOid>(json!(BASE)), vec![]).unwrap();
    let context = TaskContext::new(typed(json!(TASK)), 1).unwrap();
    let supplied: Vec<SuppliedRecord> = sorted
        .iter()
        .map(|(path, bytes, mode)| SuppliedRecord {
            path: B1RelPath::from_bytes(path.as_bytes()).unwrap(),
            origin: ChangeOrigin::Untracked,
            content: SuppliedContent::Regular {
                mode: *mode,
                bytes: bytes.to_vec(),
            },
        })
        .collect();
    (
        admit(&context, &selection, &base, &supplied, 1).unwrap(),
        context,
    )
}

const PROJECT: &[(&str, &[u8], u32)] = &[
    ("notes.txt", b"hello from the owner\n", REGULAR_MODE),
    ("remove-me.txt", b"temporary\n", REGULAR_MODE),
    ("src/main.sh", b"#!/bin/sh\necho main\n", EXECUTABLE_MODE),
];

// ---- 1. confinement is applied, verified and reported before the worker runs -------------------

#[test]
fn the_shim_applies_every_layer_reports_it_and_the_worker_has_no_privilege() {
    let rig = rig!("layers");
    let ws = rig.workspace(PROJECT);
    let plan = rig.plan(&ws);
    let p = rig.probe(&plan, &["status"]);
    assert!(p.ok(), "{p:?}");
    let d = p.detail();
    assert_eq!(d["CapEff"], "0000000000000000", "no capability");
    assert_eq!(d["CapBnd"], "0000000000000000", "none to regain");
    assert_eq!(d["NoNewPrivs"], "1");
    assert_eq!(d["Seccomp"], "2", "a filter is installed");
    let receipt = p
        .receipt
        .expect("the shim reported before it executed the worker");
    assert_eq!(receipt["confined"], true);
    assert_eq!(receipt["layers"], layer::ALL);
    assert!(receipt["landlock_abi"].as_u64().unwrap() >= 6);
    // The worker is a process of a namespace of its own: small pids, its own working directory.
    let who = rig.probe(&plan, &["who"]);
    assert!(who.detail()["pid"].as_u64().unwrap() < 20, "{who:?}");
    assert_eq!(who.detail()["cwd"], "/work");
    // Without confinement the same probe has the account's rights and a host pid.
    let free = outside(&["status"]);
    assert_ne!(free.detail()["Seccomp"], "2");
    assert_ne!(free.detail()["NoNewPrivs"], "1");
    ws.remove();
}

#[test]
fn a_required_layer_that_cannot_hold_stops_the_worker_before_it_runs() {
    let rig = rig!("abi");
    let ws = rig.workspace(PROJECT);
    let mut plan = rig.plan(&ws);
    plan.min_abi = 99;
    let ran = rig.run(
        &plan,
        "/b1/bin/probe",
        &["write", "/work/ran.txt", "the worker ran"],
    );
    assert_eq!(ran.status.code(), Some(4), "{}", ran.stderr);
    assert!(ran.stderr.contains("was not started"), "{}", ran.stderr);
    assert!(ran.stdout.trim().is_empty());
    assert!(
        !ws.path().join("ran.txt").exists(),
        "the worker must not have run"
    );
    assert!(
        ran.receipt.is_none(),
        "no 'confined' report for a refused launch"
    );
    ws.remove();
}

// ---- 2. inherited descriptors (CG-07) --------------------------------------------------------

#[test]
fn a_descriptor_the_launcher_left_open_is_closed_before_the_worker_and_open_without_the_shim() {
    let rig = rig!("fds");
    let canary_file = rig.dir("host").join("fd-canary.txt");
    fs::write(&canary_file, CANARY).unwrap();
    // A descriptor with no close-on-exec flag: what a careless launcher passes along.
    let leaked = nix::unistd::dup(std::os::fd::AsRawFd::as_raw_fd(
        &fs::File::open(&canary_file).unwrap(),
    ))
    .unwrap();
    let ws = rig.workspace(PROJECT);
    let plan = rig.plan(&ws);
    let (read, write) = nix::unistd::pipe2(nix::fcntl::OFlag::O_CLOEXEC).unwrap();
    let write = fs::File::from(write);
    let mut launch = plan
        .launch(
            &rig.shim,
            &[("probe", &rig.probe)],
            "/b1/bin/probe",
            &["read-fd".into(), leaked.to_string().into()],
            Some(&write),
        )
        .unwrap();
    let child = launch.command.spawn().unwrap();
    drop(write);
    drop(launch);
    let ran = finish(child, Duration::from_secs(30));
    let mut receipt = String::new();
    fs::File::from(read).read_to_string(&mut receipt).unwrap();
    let p = Probe::from(ran);
    assert!(p.denied(), "the worker read the leaked descriptor: {p:?}");
    assert_eq!(p.errno(), 9, "EBADF: it was closed");
    let receipt: Value = serde_json::from_str(receipt.trim()).unwrap();
    let closed: Vec<i64> = receipt["inherited_fds_closed"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_i64)
        .collect();
    assert!(
        closed.contains(&(leaked as i64)),
        "the shim saw and closed fd {leaked}: {receipt}"
    );
    // Control: the same probe started directly keeps the descriptor and reads the canary.
    let child = Command::new(PROBE)
        .args(["read-fd", &leaked.to_string()])
        .env_clear()
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let free = Probe::from(finish(child, Duration::from_secs(10)));
    assert!(
        free.ok() && free.detail().as_str().unwrap().contains(CANARY),
        "{free:?}"
    );
    let _ = nix::unistd::close(leaked);
    ws.remove();
}

// ---- 3. network (CG-16, synthetic, no packet leaves the machine) ------------------------------

#[test]
fn the_network_is_not_there_and_sockets_are_unix_only() {
    let rig = rig!("net");
    let ws = rig.workspace(PROJECT);
    let plan = rig.plan(&ws);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let address = format!("127.0.0.1:{port}");
    // Control: without confinement a connection to a local listener works.
    assert!(outside(&["tcp", &address]).ok());
    let denied = rig.probe(&plan, &["tcp", &address]);
    assert!(denied.denied(), "{denied:?}");
    // A route that does not exist, and a name nobody resolves, fail the same way.
    assert!(rig.probe(&plan, &["tcp", "93.184.216.34:80"]).denied());
    for (family, name) in [
        (2, "AF_INET"),
        (10, "AF_INET6"),
        (16, "AF_NETLINK"),
        (17, "AF_PACKET"),
    ] {
        let s = rig.probe(&plan, &["socket", &family.to_string()]);
        assert_eq!(s.errno(), 1, "{name} must be EPERM: {s:?}");
    }
    assert!(
        rig.probe(&plan, &["socket", "1"]).ok(),
        "AF_UNIX is the one family left"
    );
    ws.remove();
}

// ---- 4. the authority, other sockets, the host's files and environment -----------------------

struct Authority {
    child: Child,
    socket: PathBuf,
    run: PathBuf,
}
impl Drop for Authority {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_authority(rig: &Rig) -> Option<Authority> {
    let run = rig.dir("authority-run");
    let state = rig.dir("authority-state");
    let child = Command::new(CONTROL)
        .env_clear()
        .env("PATH", "/usr/bin")
        .env("XDG_RUNTIME_DIR", &run)
        .env("XDG_STATE_HOME", &state)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut authority = Authority {
        child,
        socket: PathBuf::new(),
        run: run.clone(),
    };
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        for dir in [run.clone(), run.join("wyss-console")] {
            if let Ok(rd) = fs::read_dir(&dir) {
                for e in rd.flatten() {
                    if e.file_type()
                        .map(|t| std::os::unix::fs::FileTypeExt::is_socket(&t))
                        .unwrap_or(false)
                    {
                        authority.socket = e.path();
                        return Some(authority);
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
    None
}

#[test]
fn the_authority_socket_is_open_to_any_process_of_the_account_and_not_to_a_b1_worker() {
    let rig = rig!("authority");
    let Some(authority) = start_authority(&rig) else {
        not_run("a private authority did not start here");
        return;
    };
    // The U boundary, as it is: a client of the same account connects and gets a session. This is
    // the accepted trust model (ADR 0011), and the reason a same-account process is not an approver.
    let client = wyss_console::control::client::ControlClient::connect_in_runtime(&authority.run);
    assert!(
        client.is_ok(),
        "same-account client could not connect: {:?}",
        client.err()
    );
    let free = outside(&["connect-unix", authority.socket.to_str().unwrap(), "{}\n"]);
    assert!(
        free.ok(),
        "control: an unconfined same-account process reaches the socket: {free:?}"
    );
    // Inside B1 the path does not exist: neither the socket, its directory nor the runtime dir.
    let ws = rig.workspace(PROJECT);
    let plan = rig.plan(&ws);
    let inside = rig.probe(
        &plan,
        &["connect-unix", authority.socket.to_str().unwrap(), "{}\n"],
    );
    assert!(inside.denied(), "{inside:?}");
    assert_eq!(
        inside.errno(),
        2,
        "ENOENT: there is no such path in the worker's view"
    );
    let run_dir = authority.run.to_str().unwrap();
    assert!(!rig.probe(&plan, &["exists", run_dir]).ok());
    // The approval channel today is weak by its own contract, and the worker has no route to it.
    assert!(!boundary::is_strong(&boundary::TODAY));
    let reachable = boundary::TODAY.reachable_from_confined_worker;
    assert!(!std::hint::black_box(reachable));
    ws.remove();
}

#[test]
fn abstract_sockets_pathname_sockets_files_and_the_environment_of_the_host_are_not_the_workers() {
    use std::os::linux::net::SocketAddrExt;
    let rig = rig!("host");
    let ws = rig.workspace(PROJECT);
    let plan = rig.plan(&ws);
    // An abstract-namespace listener.
    let name = format!("wyss-b1-test-{}", std::process::id());
    let addr = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes()).unwrap();
    let abstract_listener = UnixListener::bind_addr(&addr).unwrap();
    let accepting = thread::spawn(move || {
        let _ = abstract_listener.accept();
    });
    assert!(outside(&["connect-abstract", &name]).ok(), "control");
    let inside = rig.probe(&plan, &["connect-abstract", &name]);
    assert!(inside.denied(), "{inside:?}");
    let _ = accepting.join();
    // A pathname socket in a directory that is not bound.
    let host = rig.dir("host");
    let sock = host.join("service.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let serve = thread::spawn(move || {
        for mut s in listener.incoming().take(2).flatten() {
            let _ = s.write_all(b"PONG-FROM-HOST-SERVICE");
        }
    });
    let free = outside(&["connect-unix", sock.to_str().unwrap(), "hi"]);
    assert!(
        free.ok() && free.detail().as_str().unwrap().contains("PONG"),
        "control: {free:?}"
    );
    let inside = rig.probe(&plan, &["connect-unix", sock.to_str().unwrap(), "hi"]);
    assert_eq!(inside.errno(), 2, "{inside:?}");
    drop(TcpUnblock(sock));
    let _ = serve.join();
    // The forbidden signer: readable without B1, absent with it.
    let signer = host.join("signer.key");
    fs::write(&signer, CANARY).unwrap();
    assert!(
        outside(&["read", signer.to_str().unwrap()])
            .detail()
            .as_str()
            .unwrap()
            .contains(CANARY)
    );
    let inside = rig.probe(&plan, &["read", signer.to_str().unwrap()]);
    assert!(inside.denied() && inside.errno() == 2, "{inside:?}");
    // Home, runtime, container and bus paths are not in the worker's view at all. (Control: the ones
    // that exist on this machine are visible unconfined.)
    assert!(outside(&["exists", "/home"]).ok() && outside(&["exists", "/run"]).ok());
    for path in [
        "/home",
        "/run",
        "/run/user",
        "/var/run/docker.sock",
        "/run/docker.sock",
        "/etc/passwd",
        "/root",
        "/mnt",
    ] {
        assert!(
            !rig.probe(&plan, &["exists", path]).ok(),
            "{path} is visible"
        );
    }
    if let Some(home) = std::env::var_os("HOME") {
        assert!(!rig.probe(&plan, &["exists", home.to_str().unwrap()]).ok());
    }
    // The environment: nothing of the host's, whatever the test process carries.
    unsafe { std::env::set_var("WYSS_B1_TEST_CANARY", ENV_CANARY) };
    let env = rig.probe(&plan, &["env"]);
    let text = env.detail().to_string();
    assert!(!text.contains(ENV_CANARY), "{text}");
    for banned in [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "SSH_AUTH_SOCK",
        "XAUTHORITY",
    ] {
        assert!(!text.contains(banned), "{banned} reached the worker");
    }
    // bwrap's own environment (readable through /proc/1/environ by the worker) is empty too, and
    // the test process is not in the worker's pid namespace.
    let init = rig.probe(&plan, &["read", "/proc/1/environ"]);
    assert!(!init.detail().to_string().contains(ENV_CANARY), "{init:?}");
    let me = std::process::id().to_string();
    assert_eq!(
        rig.probe(&plan, &["read", &format!("/proc/{me}/environ")])
            .errno(),
        2
    );
    assert_eq!(
        rig.probe(&plan, &["kill", &me, "0"]).errno(),
        3,
        "ESRCH: no such process here"
    );
    assert!(
        outside(&["kill", &me, "0"]).ok(),
        "control: an unconfined process can signal it"
    );
    ws.remove();
}

/// Makes the last `accept` of a test listener return so its thread ends.
struct TcpUnblock(PathBuf);
impl Drop for TcpUnblock {
    fn drop(&mut self) {
        for _ in 0..2 {
            let _ = std::os::unix::net::UnixStream::connect(&self.0);
        }
    }
}

// ---- 5. kernel entries (seccomp) -------------------------------------------------------------

#[test]
fn the_ways_out_of_a_sandbox_end_the_process_or_are_refused() {
    let rig = rig!("seccomp");
    let ws = rig.workspace(PROJECT);
    let plan = rig.plan(&ws);
    // Control: an unconfined process of this account can make a user namespace.
    let free = outside(&["syscall", "unshare-user"]);
    assert!(free.ok(), "control: {free:?}");
    let killed = [
        "unshare-user",
        "unshare-net",
        "mount",
        "io_uring",
        "bpf",
        "keyctl",
        "process_vm_readv",
        "setns",
        "clone-newuser",
        "x32-socket",
    ];
    for action in killed {
        let p = rig.probe(&plan, &["syscall", action]);
        assert!(p.killed_by_seccomp(), "{action}: {p:?}");
        // Control: the same call without the filter is answered by the kernel, not by SIGSYS, so the
        // kill above is the filter's. (`clone-newuser` is left out: unconfined it forks the probe.)
        if action != "clone-newuser" {
            let free = outside(&["syscall", action]);
            assert!(
                !free.killed_by_seccomp() && free.value.is_some(),
                "control {action}: {free:?}"
            );
        }
    }
    // clone3 cannot be inspected, so it is refused as unsupported and libc falls back to clone.
    let p = rig.probe(&plan, &["syscall", "clone3"]);
    assert_eq!(p.errno(), 38, "ENOSYS: {p:?}");
    // Tracing another process, and making device nodes or FIFOs.
    assert!(rig.probe(&plan, &["ptrace", "1"]).killed_by_seccomp());
    assert!(!outside(&["ptrace", "1"]).killed_by_seccomp(), "control");
    assert!(
        rig.probe(&plan, &["mknod-dev", "/work/null"])
            .killed_by_seccomp()
    );
    assert!(
        rig.probe(&plan, &["mkfifo", "/work/fifo"])
            .killed_by_seccomp()
    );
    assert!(!ws.path().join("fifo").exists());
    // Control: without the filter both work for this account.
    let node = rig.dir("host").join("control-fifo");
    assert!(outside(&["mkfifo", node.to_str().unwrap()]).ok());
    // Sockets: one family only, for `socket` and for `socketpair`.
    for family in ["2", "10", "16", "17"] {
        assert_eq!(
            rig.probe(&plan, &["socketpair", family]).errno(),
            1,
            "family {family}"
        );
    }
    assert!(
        rig.probe(&plan, &["socketpair", "1"]).ok(),
        "AF_UNIX pairs are what tools use"
    );
    // What tools do need still works.
    assert!(rig.probe(&plan, &["status"]).ok());
    ws.remove();
}

// ---- 6. what the filesystem allows (Landlock + the mount namespace) ----------------------------

#[test]
fn the_worker_writes_its_workspace_and_scratch_and_nothing_else_and_executes_only_the_runtime_and_workspace()
 {
    let rig = rig!("fs");
    let ws = rig.workspace(PROJECT);
    let plan = rig.plan(&ws);
    assert!(rig.probe(&plan, &["write", "/work/out.txt", "ok"]).ok());
    assert!(rig.probe(&plan, &["write", "/tmp/scratch.txt", "ok"]).ok());
    assert!(
        rig.probe(&plan, &["symlink", "notes.txt", "/work/alias"])
            .ok(),
        "inert symlinks are data"
    );
    // Moving a file between directories of the workspace is allowed (REFER within the tree)…
    assert!(
        rig.probe(&plan, &["rename", "/work/out.txt", "/work/src/out.txt"])
            .ok()
    );
    // …and out of it is not.
    let p = rig.probe(&plan, &["rename", "/work/src/out.txt", "/tmp/out.txt"]);
    assert!(p.denied(), "{p:?}");
    for path in [
        "/usr/escape",
        "/etc/escape",
        "/escape",
        "/dev/escape",
        "/proc/escape",
    ] {
        let p = rig.probe(&plan, &["write", path, "x"]);
        assert!(p.denied(), "{path}: {p:?}");
    }
    // The runtime is read-only and cannot be linked into the workspace.
    let p = rig.probe(&plan, &["hardlink", "/usr/bin/true", "/work/true"]);
    assert!(p.denied(), "{p:?}");
    // No sockets of its own: Landlock refuses the node (MAKE_SOCK is granted nowhere).
    let p = rig.probe(&plan, &["bind-unix", "/work/s.sock"]);
    assert!(p.denied() && (p.errno() == 13 || p.errno() == 1), "{p:?}");
    assert!(!ws.path().join("s.sock").exists());
    // A program written to scratch does not run; the same program in the workspace does. One
    // sandbox for the whole sequence: its /tmp is private and gone with it.
    let in_scratch =
        "printf '#!/bin/sh\\necho hi\\n' > /tmp/run.sh && chmod +x /tmp/run.sh && /tmp/run.sh";
    let ran = rig.run(&plan, "/usr/bin/sh", &["-c", in_scratch]);
    assert_ne!(ran.status.code(), Some(0), "{} {}", ran.stdout, ran.stderr);
    assert!(
        ran.stdout.trim().is_empty(),
        "scratch executed: {}",
        ran.stdout
    );
    let in_work =
        "printf '#!/bin/sh\\necho hi\\n' > /work/run.sh && chmod +x /work/run.sh && /work/run.sh";
    let ran = rig.run(&plan, "/usr/bin/sh", &["-c", in_work]);
    assert_eq!(ran.stdout.trim(), "hi", "{}", ran.stderr);
    ws.remove();
}

// ---- 7. CG-01..03: the hardlink to a forbidden signer ----------------------------------------

#[test]
fn a_selected_file_that_was_a_hardlink_to_a_forbidden_signer_becomes_a_distinct_inode_the_worker_cannot_use()
 {
    let rig = rig!("hardlink");
    let host = rig.dir("host");
    let signer = host.join("signer.key");
    fs::write(&signer, CANARY).unwrap();
    let alias = host.join("project-file.txt");
    fs::hard_link(&signer, &alias).unwrap();
    assert_eq!(fs::metadata(&signer).unwrap().nlink(), 2);
    // The owner selected `project-file.txt`: its bytes are what was authorized, and they happen to
    // be the signer's. Materializing copies bytes into a new object.
    let bytes = fs::read(&alias).unwrap();
    let ws = rig.workspace(&[("project-file.txt", &bytes, REGULAR_MODE)]);
    let materialized = ws.path().join("project-file.txt");
    let (a, b) = (
        fs::metadata(&materialized).unwrap(),
        fs::metadata(&signer).unwrap(),
    );
    assert_ne!((a.dev(), a.ino()), (b.dev(), b.ino()), "a distinct inode");
    assert_eq!(a.nlink(), 1);
    // An in-place overwrite by the worker reaches the workspace file only.
    let plan = rig.plan(&ws);
    let p = rig.probe(
        &plan,
        &[
            "overwrite",
            "/work/project-file.txt",
            "OVERWRITTEN-BY-THE-WORKER",
        ],
    );
    assert!(p.ok(), "{p:?}");
    assert_eq!(
        fs::read_to_string(&signer).unwrap(),
        CANARY,
        "the signer is unchanged"
    );
    assert_eq!(fs::read_to_string(&alias).unwrap(), CANARY);
    // The attack control (CG-01): had the worker been handed the alias, the signer would change.
    let c = outside(&[
        "overwrite",
        alias.to_str().unwrap(),
        "OVERWRITTEN-THROUGH-THE-ALIAS",
    ]);
    assert!(c.ok());
    assert!(
        fs::read_to_string(&signer)
            .unwrap()
            .starts_with("OVERWRITTEN-THROUGH")
    );
    ws.remove();
}

#[test]
fn unselected_content_never_enters_and_a_link_that_leaves_the_tree_is_refused_before_anything_is_built()
 {
    let rig = rig!("alias");
    let ws = rig.workspace(PROJECT);
    // Every file of a fresh workspace has one name; an unselected path is simply absent.
    for e in ["notes.txt", "remove-me.txt", "src/main.sh"] {
        assert_eq!(fs::metadata(ws.path().join(e)).unwrap().nlink(), 1);
    }
    assert!(!ws.path().join("signer.key").exists());
    ws.remove();
    // A selection whose symlink target climbs out of the tree, or names an absolute path, never
    // becomes a snapshot: the contract refuses it, so nothing could be materialized from it.
    for target in [
        &b"../../escape"[..],
        b"/etc/passwd",
        b"a/../../b",
        b".git/config",
    ] {
        let link = serde_json::to_value(
            wyss_console::contracts::b1::path::B1LinkTarget::from_bytes(target).unwrap(),
        )
        .unwrap();
        let selection: B1SourceSelectionV1 = typed(json!({
            "schema_version": 1, "task_id": TASK,
            "selection_id": "22222222-2222-4222-8222-222222222222",
            "repo_id": "33333333-3333-4333-8333-333333333333",
            "base_commit_oid": BASE, "repo_object_format": "sha1",
            "admitted_overlay": [{
                "path": serde_json::to_value(B1RelPath::from_bytes(b"link").unwrap()).unwrap(),
                "operation": "add", "origin": "untracked", "expected_base_oid": null,
                "rename_from": null, "content": {"kind": "symlink", "target": link},
                "reviewed_sha256": "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"
            }],
            "policy_id": "44444444-4444-4444-8444-444444444444",
            "policy_sha256": "abababababababababababababababababababababababababababababababab",
            "issued_at": "2026-10-16T10:00:00Z"
        }));
        assert_eq!(
            selection.validate().expect_err("an unsafe link").code,
            B1ErrorCode::LinkTargetUnsafe,
            "{}",
            String::from_utf8_lossy(target)
        );
    }
}

// ---- 8. quiescence (CG-10) -------------------------------------------------------------------

#[test]
fn a_process_that_outlives_its_parent_does_not_outlive_the_sandbox() {
    let rig = rig!("quiesce");
    let ws = rig.workspace(PROJECT);
    let plan = rig.plan(&ws);
    // Control: unconfined, the detached grandchild writes after its parent is gone.
    let host = rig.dir("host");
    let late = host.join("late.txt");
    assert!(outside(&["double-fork", late.to_str().unwrap(), "800"]).ok());
    thread::sleep(Duration::from_millis(1600));
    assert!(
        late.exists(),
        "control: the detached process was still running"
    );
    // Inside: the worker's parent exits at once; the namespace goes with it.
    let p = rig.probe(&plan, &["double-fork", "/work/late.txt", "800"]);
    assert!(p.ok(), "{p:?}");
    thread::sleep(Duration::from_millis(1600));
    assert!(
        !ws.path().join("late.txt").exists(),
        "a descendant kept writing after the sandbox ended"
    );
    ws.remove();
}

// ---- 9. the positive workflow (CG-12) and what comes out -------------------------------------

#[test]
fn a_useful_edit_in_the_workspace_with_git_ends_as_typed_inert_records_in_a_quarantine_of_their_own()
 {
    let rig = rig!("workflow");
    let ws = rig.workspace(PROJECT);
    let plan = rig.plan(&ws);
    let script = r#"
set -eu
cd /work
sed -i 's/hello/hello, wyss/' notes.txt
mkdir -p out/sub
printf 'a new file\n' > out/new.txt
mv out/new.txt out/sub/new.txt
rm -f remove-me.txt
chmod +x src/main.sh
./src/main.sh
git init -q .
git add -A
git -c user.name=b1 -c user.email=b1@example.invalid commit -qm 'work'
git log --oneline | wc -l
"#;
    let ran = rig.run(&plan, "/usr/bin/sh", &["-c", script]);
    assert_eq!(
        ran.status.code(),
        Some(0),
        "stdout={} stderr={}",
        ran.stdout,
        ran.stderr
    );
    assert_eq!(ran.stdout.lines().collect::<Vec<_>>(), ["main", "1"]);
    assert!(
        ws.path().join(".git").is_dir(),
        "the worker's own repository exists"
    );

    let export = ws.export().expect("an export");
    export.verify().unwrap();
    let changes: Vec<(String, ExportOperation)> = export
        .record
        .entries
        .iter()
        .map(|e| {
            (
                String::from_utf8_lossy(e.path.as_bytes()).into_owned(),
                e.operation,
            )
        })
        .collect();
    assert_eq!(
        changes,
        [
            ("notes.txt".to_owned(), ExportOperation::Modify),
            ("out/sub/new.txt".to_owned(), ExportOperation::Add),
            ("remove-me.txt".to_owned(), ExportOperation::Delete),
        ],
        "an unchanged src/main.sh (mode aside) and the worker's .git are not in the result"
    );
    assert_eq!(
        export.skipped_git_directories, 1,
        ".git was not read, only counted"
    );
    // The mode change of the executable is a change too.
    let _ = &export;
    // Quarantine: published once, to a store that is not the source and not the workspace.
    let store = rig.dir("quarantine");
    let first = workspace::quarantine(&store, &export, &[ws.path()]).expect("published");
    let again =
        workspace::quarantine(&store, &export, &[ws.path()]).expect_err("a second publication");
    assert_eq!(again.code, B1ErrorCode::QuarantineCasConflict);
    let record = store.join(TASK).join("1").join("export.json");
    assert!(record.is_file());
    assert_eq!(first, digest::export_record(&export.record).unwrap());
    // The stored bytes are the ones the record names.
    for e in &export.record.entries {
        if let Some(wyss_console::contracts::b1::source::OverlayContent::Regular {
            content_sha256,
            ..
        }) = &e.content
        {
            let blob = fs::read(
                store
                    .join(TASK)
                    .join("1")
                    .join("blobs")
                    .join(content_sha256.as_str()),
            )
            .unwrap();
            assert_eq!(&digest::raw_sha256(&blob), content_sha256);
        }
    }
    ws.remove();
}

#[test]
fn hostile_output_is_refused_as_data_never_followed_and_never_blocks_the_exporter() {
    let rig = rig!("hostile");
    let host = rig.dir("host");
    let signer = host.join("signer.key");
    fs::write(&signer, CANARY).unwrap();
    // A link to the signer, an escaping relative link, a FIFO and a socket in the tree.
    type Make = Box<dyn Fn(&Path)>;
    let cases: [(&str, Make); 4] = [
        (
            "absolute link",
            Box::new({
                let s = signer.clone();
                move |p: &Path| std::os::unix::fs::symlink(&s, p.join("evil")).unwrap()
            }),
        ),
        (
            "escaping link",
            Box::new(|p: &Path| {
                std::os::unix::fs::symlink("../../../etc/passwd", p.join("evil")).unwrap()
            }),
        ),
        (
            "fifo",
            Box::new(|p: &Path| {
                nix::unistd::mkfifo(
                    &p.join("evil"),
                    nix::sys::stat::Mode::from_bits_truncate(0o600),
                )
                .unwrap();
            }),
        ),
        (
            "socket",
            Box::new(|p: &Path| {
                // A socket node without a listener: what a worker with the right would leave.
                nix::sys::stat::mknod(
                    &p.join("evil"),
                    nix::sys::stat::SFlag::S_IFSOCK,
                    nix::sys::stat::Mode::from_bits_truncate(0o600),
                    0,
                )
                .unwrap();
            }),
        ),
    ];
    for (what, make) in cases {
        let ws = rig.workspace(PROJECT);
        make(ws.path());
        let started = Instant::now();
        let e = ws.export().expect_err(what);
        assert_eq!(e.code, B1ErrorCode::ExportNodeUnsafe, "{what}");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{what} blocked the exporter"
        );
        assert_eq!(fs::read_to_string(&signer).unwrap(), CANARY);
        ws.remove();
    }
    // A worker-made link to the signer cannot even be followed from inside.
    let ws = rig.workspace(PROJECT);
    let plan = rig.plan(&ws);
    assert!(
        rig.probe(&plan, &["symlink", signer.to_str().unwrap(), "/work/evil"])
            .ok()
    );
    let p = rig.probe(&plan, &["read", "/work/evil"]);
    assert!(p.denied() && p.errno() == 2, "{p:?}");
    assert_eq!(
        ws.export().expect_err("absolute link").code,
        B1ErrorCode::ExportNodeUnsafe
    );
    ws.remove();
}

// ---- 10. synthetic credentials and a mediated service (CG-15/16, synthetic only) --------------

#[test]
fn a_synthetic_token_is_refreshed_in_place_and_inference_goes_through_a_broker_that_holds_the_real_one()
 {
    let rig = rig!("broker");
    let home = rig.dir("home");
    let broker_dir = rig.dir("broker");
    fs::write(
        home.join("auth.json"),
        r#"{"token":"SYNTHETIC-FAKE-TOKEN"}"#,
    )
    .unwrap();
    // The credential the worker must never see lives in the broker's memory and nowhere else.
    const REAL: &str = "SYNTHETIC-BROKER-SECRET-b81d";
    let sock = broker_dir.join("inference.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let served = Arc::new(Mutex::new(Vec::<String>::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let (served2, stop2) = (served.clone(), stop.clone());
    let server = thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        while !stop2.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut s, _)) => {
                    s.set_nonblocking(false).unwrap();
                    s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                    let mut buf = [0u8; 2048];
                    let n = s.read(&mut buf).unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..n]).into_owned();
                    // The broker adds the credential on its own side of the boundary.
                    let upstream = format!("authorization: Bearer {REAL}");
                    let answer = format!(
                        "HTTP/1.1 200 OK\r\n\r\n{{\"model\":\"synthetic\",\"upstream_headers\":{},\"worker_sent_a_credential\":{}}}",
                        upstream.len(),
                        request.contains("authorization") || request.contains(REAL)
                    );
                    served2.lock().unwrap().push(request);
                    let _ = s.write_all(answer.as_bytes());
                }
                Err(_) => thread::sleep(Duration::from_millis(10)),
            }
        }
    });
    let ws = rig.workspace(PROJECT);
    let mut plan = rig.plan(&ws);
    plan.home = Some(home.clone());
    plan.broker_dir = Some(broker_dir.clone());
    // Mediated inference: the worker names no credential and gets an answer.
    let p = rig.probe(
        &plan,
        &[
            "connect-unix",
            "/broker/inference.sock",
            "POST /v1/messages {\"prompt\":\"hi\"}",
        ],
    );
    assert!(p.ok(), "{p:?}");
    let reply = p.detail().as_str().unwrap().to_owned();
    assert!(
        reply.contains("200 OK") && reply.contains("\"worker_sent_a_credential\":false"),
        "{reply}"
    );
    assert!(
        !reply.contains(REAL),
        "the reply must not carry the credential either"
    );
    // The real credential is nowhere the worker can look.
    let env = rig.probe(&plan, &["env"]);
    assert!(!env.detail().to_string().contains(REAL));
    for path in ["/proc/1/environ", "/home/b1/auth.json"] {
        assert!(
            !rig.probe(&plan, &["read", path])
                .detail()
                .to_string()
                .contains(REAL),
            "{path}"
        );
    }
    assert!(rig.probe(&plan, &["list", "/home/b1"]).ok());
    // The provider's own refresh pattern (write beside, rename over) works in the private home.
    let p = rig.probe(
        &plan,
        &[
            "atomic-auth",
            "/home/b1",
            r#"{"token":"SYNTHETIC-REFRESHED"}"#,
        ],
    );
    assert!(p.ok(), "{p:?}");
    assert_eq!(
        fs::read_to_string(home.join("auth.json")).unwrap(),
        r#"{"token":"SYNTHETIC-REFRESHED"}"#
    );
    // A swap for a link to a host file leads nowhere inside.
    let outside_file = rig.dir("host").join("real-credential.json");
    fs::write(&outside_file, REAL).unwrap();
    assert!(
        rig.probe(
            &plan,
            &[
                "symlink",
                outside_file.to_str().unwrap(),
                "/home/b1/alias.json"
            ]
        )
        .ok()
    );
    assert_eq!(
        rig.probe(&plan, &["read", "/home/b1/alias.json"]).errno(),
        2
    );
    assert_eq!(fs::read_to_string(&outside_file).unwrap(), REAL);
    // The broker's memory is not reachable: its pid is not in the worker's namespace.
    let me = std::process::id().to_string();
    assert_eq!(
        rig.probe(&plan, &["read", &format!("/proc/{me}/mem")])
            .errno(),
        2
    );
    stop.store(true, Ordering::SeqCst);
    server.join().unwrap();
    assert_eq!(served.lock().unwrap().len(), 1);
    ws.remove();
}

// ---- 11. rootful delegation and the account (facts, never connected) --------------------------

#[test]
fn the_container_daemon_and_admin_delegation_are_reported_as_facts_and_are_not_in_the_workers_view()
{
    let rig = rig!("admin");
    let ws = rig.workspace(PROJECT);
    let plan = rig.plan(&ws);
    // Facts about this account, read, never used: membership of a group that can drive a rootful
    // daemon makes everything B1 does irrelevant to an attacker who is *outside* B1, and B1 does
    // not claim otherwise.
    let groups = fs::read_to_string("/proc/self/status").unwrap();
    let ids: Vec<u32> = groups
        .lines()
        .find_map(|l| l.strip_prefix("Groups:"))
        .unwrap_or("")
        .split_whitespace()
        .filter_map(|g| g.parse().ok())
        .collect();
    let docker_gid = fs::read_to_string("/etc/group")
        .unwrap_or_default()
        .lines()
        .find_map(|l| {
            let mut f = l.split(':');
            (f.next() == Some("docker"))
                .then(|| f.nth(1).and_then(|g| g.parse::<u32>().ok()))
                .flatten()
        });
    let in_docker_group = docker_gid.is_some_and(|g| ids.contains(&g));
    let socket = Path::new("/var/run/docker.sock");
    eprintln!(
        "FACT: docker_group_exists={} account_in_docker_group={} docker_socket_present={}",
        docker_gid.is_some(),
        in_docker_group,
        socket.exists()
    );
    // Whatever those facts are, the worker has no path to any container or bus socket.
    for path in [
        "/var/run/docker.sock",
        "/run/docker.sock",
        "/run/podman/podman.sock",
        "/run/dbus/system_bus_socket",
        "/run/user/1000/bus",
    ] {
        assert!(!rig.probe(&plan, &["exists", path]).ok(), "{path}");
        let c = rig.probe(&plan, &["connect-unix", path, "x"]);
        assert!(c.denied() && c.errno() == 2, "{path}: {c:?}");
    }
    // sudo and setuid helpers cannot raise anything inside: no new privileges, no capabilities.
    if Path::new("/usr/bin/sudo").exists() {
        let ran = rig.run(&plan, "/usr/bin/sudo", &["-n", "true"]);
        assert_ne!(ran.status.code(), Some(0));
        assert!(!ran.stdout.contains("root"), "{}", ran.stdout);
    } else {
        not_run("no sudo on this machine to try");
    }
    ws.remove();
}

// ---- 12. plans that must not be made ---------------------------------------------------------

#[test]
fn a_plan_over_the_account_itself_or_with_a_helper_anyone_can_edit_is_refused() {
    let rig = Rig::new("refuse");
    let probe = |ws: PathBuf| {
        Plan::new(ws)
            .launch(&rig.shim, &[], "/usr/bin/true", &[], None)
            .err()
    };
    let private_home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|h| fs::metadata(h).is_ok_and(|m| m.permissions().mode() & 0o077 == 0));
    if let Some(home) = private_home {
        assert_eq!(probe(home), Some(PlanError::TooBroad));
    }
    assert!(probe(PathBuf::from("/")).is_some());
    assert!(probe(PathBuf::from("/usr")).is_some());
    // A shim that is writable by the group is not run.
    let ws = rig.dir("ws");
    let weak = rig.root.join("bin").join("weak-shim");
    fs::copy(SHIM, &weak).unwrap();
    fs::set_permissions(&weak, fs::Permissions::from_mode(0o775)).unwrap();
    assert_eq!(
        Plan::new(ws)
            .launch(&weak, &[], "/usr/bin/true", &[], None)
            .err(),
        Some(PlanError::HelperNotTrusted)
    );
}

#[test]
fn the_digest_the_tests_rely_on_matches_the_published_sha256_test_vector() {
    let sha: Sha256Hex = digest::raw_sha256(b"abc");
    assert_eq!(
        sha.as_str(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

// ---- 13. the exporter and the quarantine, without a sandbox (they run after the worker) --------

use wyss_console::b1::workspace::Export;

/// A workspace edited the way a finished worker would have, and its export.
fn edited(rig: &Rig) -> (Workspace, Export) {
    let ws = rig.workspace(PROJECT);
    fs::write(ws.path().join("notes.txt"), "edited\n").unwrap();
    fs::create_dir_all(ws.path().join("out")).unwrap();
    fs::write(ws.path().join("out/new.txt"), "new\n").unwrap();
    fs::remove_file(ws.path().join("remove-me.txt")).unwrap();
    let export = ws.export().expect("an export");
    export.verify().unwrap();
    (ws, export)
}

#[test]
fn the_quarantine_takes_exactly_the_content_the_record_names_and_writes_nothing_before_it_has_checked()
 {
    let rig = Rig::new("blobs");
    let (ws, export) = edited(&rig);
    let store = rig.dir("quarantine");
    let extra = digest::raw_sha256(b"not named by any entry");
    let mut with_extra = export.blobs.clone();
    with_extra.push((extra.clone(), b"not named by any entry".to_vec()));
    let bad = |blobs: Vec<(Sha256Hex, Vec<u8>)>| Export {
        record: export.record.clone(),
        blobs,
        skipped_git_directories: 0,
    };
    // An extra blob, a missing one, a duplicate and a blob whose bytes are not its digest.
    let mut duplicated = export.blobs.clone();
    duplicated.push(export.blobs[0].clone());
    let mut corrupt = export.blobs.clone();
    corrupt[0].1.push(b'!');
    let mut missing = export.blobs.clone();
    missing.pop();
    for (what, blobs) in [
        ("extra", with_extra),
        ("duplicate", duplicated),
        ("corrupt", corrupt),
        ("missing", missing),
    ] {
        let e = workspace::quarantine(&store, &bad(blobs), &[ws.path()]).expect_err(what);
        assert!(
            matches!(
                e.code,
                B1ErrorCode::ExportDigestMismatch
                    | B1ErrorCode::ContentDigestMismatch
                    | B1ErrorCode::ContentSizeMismatch
            ),
            "{what}: {e}"
        );
        assert_eq!(
            fs::read_dir(&store).unwrap().count(),
            0,
            "{what}: something was written before the check"
        );
    }
    // The real one goes in, once, with the record in place by rename (no half-written record).
    workspace::quarantine(&store, &export, &[ws.path()]).unwrap();
    let dir = store.join(TASK).join("1");
    assert!(dir.join("export.json").is_file());
    assert!(!dir.join("export.json.partial").exists());
    let mut names: Vec<_> = fs::read_dir(dir.join("blobs"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    let mut expected: Vec<_> = export
        .blobs
        .iter()
        .map(|(s, _)| s.as_str().to_owned())
        .collect();
    expected.sort();
    assert_eq!(names, expected, "nothing else is in the store");
    ws.remove();
}

#[test]
fn the_quarantine_does_not_follow_a_link_in_its_store_and_stays_apart_from_the_workspace() {
    let rig = Rig::new("store");
    let (ws, export) = edited(&rig);
    let elsewhere = rig.dir("elsewhere");
    // A store that is a link is not a store.
    let link = rig.root.join("store-link");
    std::os::unix::fs::symlink(&elsewhere, &link).unwrap();
    assert!(workspace::quarantine(&link, &export, &[ws.path()]).is_err());
    // A task directory that is a link inside a real store leads nowhere.
    let store = rig.dir("quarantine");
    std::os::unix::fs::symlink(&elsewhere, store.join(TASK)).unwrap();
    assert!(workspace::quarantine(&store, &export, &[ws.path()]).is_err());
    assert_eq!(
        fs::read_dir(&elsewhere).unwrap().count(),
        0,
        "nothing was written through a link"
    );
    // A store inside the workspace, or around it, is refused.
    let inside = ws.path().join("store");
    fs::DirBuilder::new().mode(0o700).create(&inside).unwrap();
    assert!(workspace::quarantine(&inside, &export, &[ws.path()]).is_err());
    assert!(workspace::quarantine(&rig.root, &export, &[ws.path()]).is_err());
    ws.remove();
}

#[test]
fn only_one_of_many_concurrent_publishers_of_a_task_and_generation_wins() {
    let rig = Rig::new("cas");
    let (ws, export) = edited(&rig);
    let store = rig.dir("quarantine");
    let export = Arc::new(export);
    let results: Vec<_> = (0..8)
        .map(|_| {
            let (store, export) = (store.clone(), export.clone());
            thread::spawn(move || workspace::quarantine(&store, &export, &[]))
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect();
    let won = results.iter().filter(|r| r.is_ok()).count();
    assert_eq!(won, 1, "{results:?}");
    for r in results.iter().filter(|r| r.is_err()) {
        let code = r.as_ref().unwrap_err().code;
        assert!(
            matches!(
                code,
                B1ErrorCode::QuarantineCasConflict | B1ErrorCode::QuarantineRecoveryNeeded
            ),
            "{code:?}"
        );
    }
    // The winner's publication is whole.
    assert!(store.join(TASK).join("1").join("export.json").is_file());
    ws.remove();
}

#[test]
fn a_tree_of_millions_of_nodes_is_refused_by_count_before_it_is_read() {
    let rig = Rig::new("nodes");
    let ws = rig.workspace(PROJECT);
    // More directories than any export may contain, all empty: they are not entries, but they are
    // work, and the exporter counts them.
    let many = ws.path().join("many");
    fs::create_dir(&many).unwrap();
    for i in 0..17_000u32 {
        fs::create_dir(many.join(format!("d{i}"))).unwrap();
    }
    let started = Instant::now();
    let e = ws.export().expect_err("too many nodes");
    assert_eq!(e.code, B1ErrorCode::LimitExceeded);
    assert!(started.elapsed() < Duration::from_secs(10));
    ws.remove();
}

#[test]
fn a_plan_made_for_a_workspace_refuses_a_different_directory_at_the_same_path() {
    let rig = Rig::new("swap");
    let ws = rig.workspace(PROJECT);
    let plan = rig.plan(&ws);
    assert!(
        plan.launch(&rig.shim, &[], "/usr/bin/true", &[], None)
            .is_ok()
    );
    assert!(
        plan.min_abi >= confine::MIN_LANDLOCK_ABI,
        "the default floor is ABI 6"
    );
    // Move the real tree away and put another private directory where it was.
    let away = rig.root.join("moved-away");
    fs::rename(ws.path(), &away).unwrap();
    fs::DirBuilder::new().mode(0o700).create(ws.path()).unwrap();
    assert_eq!(
        plan.launch(&rig.shim, &[], "/usr/bin/true", &[], None)
            .err(),
        Some(PlanError::WorkspaceChanged)
    );
    fs::remove_dir(ws.path()).unwrap();
    fs::rename(&away, ws.path()).unwrap();
    ws.remove();
}
