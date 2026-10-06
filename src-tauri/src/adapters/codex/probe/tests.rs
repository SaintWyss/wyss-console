use super::*;
use serde_json::{Value, json};
use std::{collections::VecDeque, os::unix::process::CommandExt};

#[derive(Clone, Copy, Default)]
enum Scenario {
    #[default]
    Happy,
    CatalogError,
    InitError,
    InvalidInit,
    InvalidCatalog,
    Notification,
    ServerRequest,
    Unrelated,
    Duplicate,
    Timeout,
    EarlyEof,
    LateDuplicate,
    LateServerRequest,
}
#[derive(Default)]
struct FakeIo {
    sent: Vec<Value>,
    queue: VecDeque<WireMessage>,
    scenario: Scenario,
    first_id: Option<Value>,
    closed: bool,
}
fn inbound(value: Value) -> WireMessage {
    WireMessage::from_value(value, Direction::ServerToClient).unwrap()
}
fn init_result() -> Value {
    json!({"userAgent":"fixture","codexHome":"/synthetic","platformFamily":"unix","platformOs":"linux"})
}
fn catalog() -> Value {
    json!({"data":[{"id":"runtime-model","model":"runtime-model","displayName":"Fixture model","description":"Synthetic",
        "hidden":false,"isDefault":true,"defaultReasoningEffort":"medium","supportedReasoningEfforts":[{"reasoningEffort":"medium","description":"Fixture"}]}],"nextCursor":"bounded-next-page"})
}
impl ProbeIo for FakeIo {
    fn send(&mut self, bytes: &[u8], _: Instant) -> ProbeResult<()> {
        assert_eq!(bytes.last(), Some(&b'\n'));
        let m: Value = serde_json::from_slice(bytes).unwrap();
        let method = m["method"].as_str().unwrap();
        if method == "initialize" {
            self.first_id = Some(m["id"].clone());
            match self.scenario {
                Scenario::Notification => self.queue.push_back(inbound(
                    json!({"method":"future/notice","params":{"untrusted":"not retained"}}),
                )),
                Scenario::ServerRequest => self.queue.push_back(inbound(
                    json!({"id":"server-1","method":"future/action","params":{}}),
                )),
                Scenario::Unrelated => self.queue.push_back(inbound(json!({"id":17,"result":{}}))),
                _ => {}
            }
            if !matches!(self.scenario, Scenario::EarlyEof | Scenario::Timeout) {
                let response = match self.scenario {
                    Scenario::InitError => {
                        json!({"id":m["id"],"error":{"code":-32600,"message":"synthetic private token-looking string"}})
                    }
                    Scenario::InvalidInit => json!({"id":m["id"],"result":{}}),
                    _ => json!({"id":m["id"],"result":init_result()}),
                };
                self.queue.push_back(inbound(response));
            }
        } else if method == "model/list" {
            if matches!(self.scenario, Scenario::Duplicate) {
                self.queue
                    .push_back(inbound(json!({"id":self.first_id,"result":init_result()})));
            }
            let response = match self.scenario {
                Scenario::CatalogError => {
                    json!({"id":m["id"],"error":{"code":-32600,"message":"api-key synthetic-secret /private/config","data":{"untrusted":"secret"}}})
                }
                Scenario::InvalidCatalog => json!({"id":m["id"],"result":{"data":false}}),
                _ => json!({"id":m["id"],"result":catalog()}),
            };
            self.queue.push_back(inbound(response.clone()));
            if matches!(self.scenario, Scenario::LateDuplicate) {
                self.queue.push_back(inbound(response));
            }
            if matches!(self.scenario, Scenario::LateServerRequest) {
                self.queue
                    .push_back(inbound(json!({"id":1,"method":"future/request"})));
            }
        }
        self.sent.push(m);
        Ok(())
    }
    fn receive(&mut self, _: Instant) -> ProbeResult<Option<WireMessage>> {
        if matches!(self.scenario, Scenario::Timeout) {
            return Err(ProbeError::Timeout);
        }
        Ok(self.queue.pop_front())
    }
    fn close_input(&mut self) {
        self.closed = true;
    }
}
fn run_fake(scenario: Scenario) -> (CodexLiveProbeReportV1, FakeIo, ProbeResult<()>) {
    let mut io = FakeIo {
        scenario,
        ..Default::default()
    };
    let mut report = initial_report(PathBuf::from("/synthetic"));
    let result = exchange(&mut io, &mut report);
    if let Err(e) = result {
        apply_failure(&mut report, e);
    }
    (report, io, result)
}
#[test]
fn probe_plan_has_exact_read_only_allowlist_and_no_work() {
    let (_, io, result) = run_fake(Scenario::Happy);
    result.unwrap();
    assert_eq!(
        io.sent
            .iter()
            .map(|m| m["method"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["initialize", "initialized", "model/list"]
    );
    assert_eq!(
        io.sent[0]["params"]["capabilities"],
        json!({"experimentalApi":false})
    );
    assert_eq!(
        io.sent[0]["params"]["clientInfo"],
        json!({"name":"wyss_console","title":"Wyss Console","version":env!("CARGO_PKG_VERSION")})
    );
    assert_eq!(
        io.sent[2]["params"],
        json!({"limit":50,"cursor":null,"includeHidden":false})
    );
    assert!(
        io.sent
            .iter()
            .all(|v| v.get("jsonrpc").is_none() && v.get("result").is_none())
    );
    for forbidden in [
        "thread/start",
        "thread/resume",
        "turn/start",
        "turn/interrupt",
        "prompt",
        "input",
        "approval",
    ] {
        assert!(!serde_json::to_string(&io.sent).unwrap().contains(forbidden));
    }
    assert!(io.closed);
}
#[test]
fn fresh_request_ids_and_exact_type_preservation() {
    let a = ProbePlan::new().unwrap();
    let b = ProbePlan::new().unwrap();
    assert_ne!(a.initialize.id(), a.models.id());
    assert_ne!(a.initialize.id(), b.initialize.id());
    for id in [
        CodexRequestId::Integer(i64::MIN),
        CodexRequestId::String("-9223372036854775808".into()),
    ] {
        let mut io = FakeIo::default();
        io.queue
            .push_back(inbound(json!({"id":id,"result":init_result()})));
        let mut c = Correlation {
            completed: vec![],
            notification_count: 0,
        };
        let mut report = initial_report("/synthetic".into());
        assert_eq!(
            c.response(&mut io, &id, Operation::Initialize, &mut report)
                .unwrap()
                .id(),
            Some(&id)
        );
    }
}
#[test]
fn compatible_catalog_normalizes_without_provider_or_entitlement_guess() {
    let (r, _, result) = run_fake(Scenario::Happy);
    result.unwrap();
    assert_eq!(r.compatibility, CompatibilityState::Compatible);
    assert!(r.initialized_sent);
    assert_eq!(r.models.len(), 1);
    assert!(r.next_cursor_present);
    let value = serde_json::to_value(&r.models[0].model).unwrap();
    assert_eq!(value["provider_id"], Value::Null);
    assert_eq!(value["availability"], "unknown");
    assert_eq!(
        r.confirmed_evidence,
        [
            "structured_stdio",
            "initialize_handshake",
            "model_list.observe"
        ]
    );
}
#[test]
fn catalog_error_degrades_without_leaking_or_guessing_authentication() {
    let (r, _, result) = run_fake(Scenario::CatalogError);
    result.unwrap();
    assert_eq!(r.compatibility, CompatibilityState::Degraded);
    assert_eq!(r.vendor_error_code, Some(-32600));
    assert!(!r.confirmed_evidence.contains(&"model_list.observe"));
    let output = serde_json::to_string(&r).unwrap();
    for private in [
        "api-key",
        "synthetic-secret",
        "/private/config",
        "untrusted",
    ] {
        assert!(!output.contains(private));
    }
}
#[test]
fn notifications_interleave_and_only_method_counts_survive() {
    let (r, _, result) = run_fake(Scenario::Notification);
    result.unwrap();
    assert_eq!(r.notifications.get("future/notice"), Some(&1));
    assert!(!serde_json::to_string(&r).unwrap().contains("not retained"));
}
#[test]
fn initialize_error_never_sends_initialized_or_models() {
    let (r, io, result) = run_fake(Scenario::InitError);
    assert!(result.is_err());
    assert_eq!(io.sent.len(), 1);
    assert!(!r.initialized_sent);
}
#[test]
fn malformed_results_are_incompatible_without_relaxing_codec() {
    for scenario in [Scenario::InvalidInit, Scenario::InvalidCatalog] {
        let (r, _, result) = run_fake(scenario);
        assert_eq!(result, Err(ProbeError::Protocol));
        assert_eq!(r.compatibility, CompatibilityState::Incompatible);
    }
}
#[test]
fn unrelated_and_duplicate_responses_reject_including_final_drain() {
    for (scenario, error) in [
        (Scenario::Unrelated, ProbeError::UnrelatedResponse),
        (Scenario::Duplicate, ProbeError::DuplicateResponse),
        (Scenario::LateDuplicate, ProbeError::DuplicateResponse),
    ] {
        assert_eq!(run_fake(scenario).2, Err(error));
    }
}
#[test]
fn server_requests_are_never_answered_even_during_shutdown() {
    for s in [Scenario::ServerRequest, Scenario::LateServerRequest] {
        let (r, io, result) = run_fake(s);
        assert_eq!(result, Err(ProbeError::UnexpectedServerRequest));
        assert!(r.unexpected_server_request);
        assert!(
            io.sent
                .iter()
                .all(|m| m.get("result").is_none() && m.get("error").is_none())
        );
    }
}
#[test]
fn timeouts_and_early_exit_stop_without_post_init_operations() {
    for (s, error) in [
        (Scenario::Timeout, ProbeError::Timeout),
        (Scenario::EarlyEof, ProbeError::UnexpectedEof),
    ] {
        let (r, io, result) = run_fake(s);
        assert_eq!(result, Err(error));
        assert_eq!(io.sent.len(), 1);
        assert_eq!(r.compatibility, CompatibilityState::Unknown);
    }
}
#[test]
fn notification_metadata_and_count_are_bounded() {
    let mut c = Correlation {
        completed: vec![],
        notification_count: 0,
    };
    let mut r = initial_report("/fixture".into());
    for i in 0..METHOD_LIMIT {
        c.observe(&inbound(json!({"method":format!("unknown/{i}")})), &mut r)
            .unwrap();
    }
    assert_eq!(
        c.observe(&inbound(json!({"method":"one/too_many"})), &mut r),
        Err(ProbeError::NotificationLimit)
    );
    let mut c = Correlation {
        completed: vec![],
        notification_count: NOTIFICATION_LIMIT,
    };
    assert_eq!(
        c.observe(&inbound(json!({"method":"unknown/0"})), &mut r),
        Err(ProbeError::NotificationLimit)
    );
}
#[test]
fn unavailable_is_unknown_not_incompatible() {
    assert!(discover(None).is_none());
    assert!(
        discover(Some(std::ffi::OsStr::new(
            "relative:/wyss-nonexistent-fixture"
        )))
        .is_none()
    );
    let r = initial_report("/fixture".into());
    assert_eq!(r.compatibility, CompatibilityState::Unknown);
}
#[test]
fn spawn_failure_is_structured() {
    assert!(matches!(
        OwnedProcess::spawn(&mut Command::new("/wyss-nonexistent-fixture")),
        Err(ProbeError::Spawn)
    ));
}

fn cat() -> OwnedProcess {
    OwnedProcess::spawn(&mut Command::new("/bin/cat")).unwrap()
}
fn alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}
#[test]
fn real_owned_pipes_frame_multiple_messages_and_reap_on_eof() {
    let mut child = cat();
    let pid = child.pid();
    let mut io = Stdio {
        owner: &mut child,
        frames: Frames::default(),
        rejected: None,
    };
    io.send(
        b"{\"method\":\"unknown/one\"}\n{\"id\":7,\"result\":{}}\n",
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(
        io.receive(Instant::now() + Duration::from_secs(2))
            .unwrap()
            .unwrap()
            .method(),
        Some("unknown/one")
    );
    assert_eq!(
        io.receive(Instant::now() + Duration::from_secs(2))
            .unwrap()
            .unwrap()
            .id(),
        Some(&CodexRequestId::Integer(7))
    );
    io.close_input();
    assert!(
        io.receive(Instant::now() + Duration::from_secs(2))
            .unwrap()
            .is_none()
    );
    let report = child.shutdown().unwrap();
    assert!(report.reaped && report.stdin_closed);
    assert!(!report.terminated);
    assert!(!alive(pid));
}
#[test]
fn byte_fragmentation_preserves_protocol_and_unicode() {
    let mut child = cat();
    let mut io = Stdio {
        owner: &mut child,
        frames: Frames::default(),
        rejected: None,
    };
    for b in "{\"method\":\"future/value\",\"params\":{\"text\":\"日本✓\"}}\n".as_bytes() {
        io.send(&[*b], Instant::now() + Duration::from_secs(2))
            .unwrap();
    }
    let m = io
        .receive(Instant::now() + Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(m.params().unwrap()["text"], "日本✓");
    child.shutdown().unwrap();
}
#[test]
fn corrupt_stdout_is_not_skipped() {
    for bytes in [b"startup banner\n".as_slice(), b"{bad}\n", b"\n"] {
        let mut child = cat();
        let mut io = Stdio {
            owner: &mut child,
            frames: Frames::default(),
            rejected: None,
        };
        io.send(bytes, Instant::now() + Duration::from_secs(2))
            .unwrap();
        assert_eq!(
            io.receive(Instant::now() + Duration::from_secs(2))
                .unwrap_err(),
            ProbeError::WireRejected("invalid_json")
        );
        child.shutdown().unwrap();
    }
}
#[test]
fn unterminated_stdout_frame_is_corruption() {
    let mut child = cat();
    let mut io = Stdio {
        owner: &mut child,
        frames: Frames::default(),
        rejected: None,
    };
    io.send(b"{}", Instant::now() + Duration::from_secs(2))
        .unwrap();
    io.close_input();
    assert_eq!(
        io.receive(Instant::now() + Duration::from_secs(2))
            .unwrap_err(),
        ProbeError::UnexpectedEof
    );
    child.shutdown().unwrap();
}
#[test]
fn oversize_frame_rejects_before_unbounded_allocation() {
    let mut child =
        OwnedProcess::spawn(Command::new("/usr/bin/head").args(["-c", "262145", "/dev/zero"]))
            .unwrap();
    let mut io = Stdio {
        owner: &mut child,
        frames: Frames::default(),
        rejected: None,
    };
    assert_eq!(
        io.receive(Instant::now() + Duration::from_secs(2))
            .unwrap_err(),
        ProbeError::OversizedFrame
    );
    assert_eq!(io.frames.partial.len(), MAX_FRAME_BYTES);
    assert!(io.frames.partial.capacity() <= MAX_FRAME_BYTES);
    child.shutdown().unwrap();
}
#[test]
fn version_success_and_exit_status() {
    let mut child =
        OwnedProcess::spawn(Command::new("/usr/bin/printf").arg("codex-cli 1.2.3\n")).unwrap();
    assert_eq!(version(&mut child).unwrap(), "codex-cli 1.2.3");
    assert_eq!(child.shutdown().unwrap().exit_code, Some(0));
}
#[test]
fn version_output_is_bounded() {
    let mut child =
        OwnedProcess::spawn(Command::new("/usr/bin/head").args(["-c", "4097", "/dev/zero"]))
            .unwrap();
    assert_eq!(version(&mut child), Err(ProbeError::VersionOutput));
    child.shutdown().unwrap();
}
#[test]
fn deadline_timeout_does_not_orphan_child() {
    let mut child = OwnedProcess::spawn(Command::new("/bin/sleep").arg("60")).unwrap();
    let pid = child.pid();
    let mut io = Stdio {
        owner: &mut child,
        frames: Frames::default(),
        rejected: None,
    };
    assert_eq!(
        io.receive(Instant::now() + Duration::from_millis(20))
            .unwrap_err(),
        ProbeError::Timeout
    );
    let shutdown = child.shutdown().unwrap();
    assert!(shutdown.terminated && shutdown.reaped);
    assert!(!alive(pid));
}
#[test]
fn dropped_owner_reaps_only_its_child() {
    let other = OwnedProcess::spawn(Command::new("/bin/sleep").arg("60")).unwrap();
    let other_pid = other.pid();
    let pid;
    {
        let child = OwnedProcess::spawn(Command::new("/bin/sleep").arg("60")).unwrap();
        pid = child.pid();
    }
    assert!(!alive(pid));
    assert!(alive(other_pid));
    drop(other);
    assert!(!alive(other_pid));
}
#[test]
fn stderr_flood_is_consumed_and_retention_bounded() {
    let mut cmd = Command::new("/usr/bin/head");
    cmd.args(["-c", "200000", "/dev/zero"]);
    // Test fixture only: dup2 is async-signal-safe between fork/exec.
    unsafe {
        cmd.pre_exec(|| {
            if nix::libc::dup2(2, 1) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = OwnedProcess::spawn(&mut cmd).unwrap();
    let mut scratch = [0; 4096];
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.read(&mut scratch).unwrap() {
            Some(0) => break,
            Some(_) => {}
            None => child.wait(deadline, false).unwrap(),
        }
    }
    let shutdown = child.shutdown().unwrap();
    assert!(shutdown.reaped);
    assert_eq!(child.stderr_bytes, 200000);
}
#[test]
fn sigterm_resistant_owned_child_is_killed_and_reaped() {
    let mut cmd = Command::new("/bin/sleep");
    cmd.arg("60");
    // Test fixture only: inherited ignored SIGTERM survives exec. No Rust allocation.
    unsafe {
        cmd.pre_exec(|| {
            nix::libc::signal(nix::libc::SIGTERM, nix::libc::SIG_IGN);
            Ok(())
        });
    }
    let mut child = OwnedProcess::spawn(&mut cmd).unwrap();
    let pid = child.pid();
    let shutdown = child.shutdown().unwrap();
    assert!(shutdown.terminated && shutdown.killed && shutdown.reaped);
    assert!(!alive(pid));
}

#[test]
fn discovery_canonicalizes_first_executable_and_ignores_relative_entries() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/probe-discovery")
        .join(Uuid::new_v4().to_string());
    fs::create_dir_all(&root).unwrap();
    let executable = root.join("codex-real");
    fs::write(&executable, b"synthetic never executed").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    std::os::unix::fs::symlink(&executable, root.join("codex")).unwrap();
    let path = env::join_paths([Path::new("relative"), root.as_path()]).unwrap();
    assert_eq!(discover(Some(&path)), Some(executable.clone()));
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(discover(Some(&path)), None);
    fs::remove_file(root.join("codex")).unwrap();
    fs::remove_file(executable).unwrap();
    fs::remove_dir(root).unwrap();
}
#[test]
fn actual_version_timeout_reaps_owned_process() {
    let mut child = OwnedProcess::spawn(Command::new("/bin/sleep").arg("60")).unwrap();
    let pid = child.pid();
    assert_eq!(
        version_until(&mut child, Instant::now() + Duration::from_millis(20)),
        Err(ProbeError::Timeout)
    );
    assert!(child.shutdown().unwrap().reaped);
    assert!(!alive(pid));
}
#[test]
fn blocked_stdin_write_has_deadline_and_bounded_input() {
    let mut child = OwnedProcess::spawn(Command::new("/bin/sleep").arg("60")).unwrap();
    assert_eq!(
        child.write(
            &vec![0; MAX_FRAME_BYTES],
            Instant::now() + Duration::from_millis(20)
        ),
        Err(ProbeError::Timeout)
    );
    assert!(child.shutdown().unwrap().reaped);
}
#[test]
fn stderr_tail_cannot_exceed_its_fixed_bound() {
    let mut child = cat();
    child.retain_stderr(&vec![1; process::STDERR_LIMIT + 100]);
    assert_eq!(child.tail_len(), process::STDERR_LIMIT);
    assert_eq!(child.stderr_bytes, (process::STDERR_LIMIT + 100) as u64);
    child.shutdown().unwrap();
    assert_eq!(child.tail_len(), 0);
}

#[test]
fn unclassified_initialize_runtime_error_is_not_proof_of_incompatibility() {
    let (r, _, result) = run_fake(Scenario::InitError);
    assert_eq!(result, Err(ProbeError::RuntimeRejected));
    assert_eq!(r.compatibility, CompatibilityState::Unknown);
    assert!(r.confirmed_evidence.is_empty());
}
#[test]
fn unverified_notification_field_still_rejects_with_structural_diagnostic() {
    // Synthetic unverified extension, not captured traffic. Official emission
    // metadata must not create an escape hatch for arbitrary envelope fields.
    let mut child = cat();
    let mut io = Stdio {
        owner: &mut child,
        frames: Frames::default(),
        rejected: None,
    };
    io.send(
        b"{\"method\":\"future/notice\",\"params\":{},\"synthetic_extra\":true}\n",
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(
        io.receive(Instant::now() + Duration::from_secs(2))
            .unwrap_err(),
        ProbeError::WireRejected("unknown_field")
    );
    let summary = io.rejected.as_ref().unwrap();
    assert_eq!(summary.category_candidate, "notification");
    assert!(
        summary
            .top_level_keys
            .iter()
            .any(|k| k == "synthetic_extra")
    );
    child.shutdown().unwrap();
}
#[test]
fn stdio_preserves_official_notification_metadata_without_payload_diagnostics() {
    let mut child = cat();
    let mut io = Stdio {
        owner: &mut child,
        frames: Frames::default(),
        rejected: None,
    };
    io.send(
        br#"{"method":"future/notice","params":{"private":"not retained"},"emittedAtMs":9223372036854775807}
"#,
        Instant::now() + Duration::from_secs(2),
    ).unwrap();
    let message = io
        .receive(Instant::now() + Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(message.emitted_at_ms(), Some(i64::MAX));
    let mut report = initial_report("/fixture".into());
    Correlation {
        completed: vec![],
        notification_count: 0,
    }
    .observe(&message, &mut report)
    .unwrap();
    assert!(report.notification_shapes["future/notice"].emitted_at_ms_present);
    let safe = serde_json::to_string(&report).unwrap();
    assert!(!safe.contains("not retained"));
    assert!(!safe.contains("9223372036854775807"));
    assert!(io.rejected.is_none());
    child.shutdown().unwrap();
}
#[test]
fn version_nonzero_exit_is_observable() {
    let mut child = OwnedProcess::spawn(&mut Command::new("/bin/false")).unwrap();
    assert_eq!(version(&mut child), Err(ProbeError::VersionOutput));
    assert_eq!(child.shutdown().unwrap().exit_code, Some(1));
}

#[test]
fn harness_success_requires_finished_probe_and_cleanup_not_only_handshake() {
    let (mut report, _, result) = run_fake(Scenario::CatalogError);
    result.unwrap();
    report.shutdown = Some(ShutdownReport {
        reaped: true,
        stdin_closed: true,
        ..Default::default()
    });
    assert!(report.successful_probe());
    report.model_list_result = ProbeStage::Failed;
    apply_failure(&mut report, ProbeError::Timeout);
    assert!(!report.successful_probe());
}
#[test]
fn parsed_version_requires_three_numeric_components_without_affecting_compatibility() {
    assert_eq!(parse_version("codex-cli 0.159.0"), Some("0.159.0".into()));
    for raw in [
        "codex-cli .",
        "codex-cli 1..2",
        "codex-cli 1.2",
        "codex-cli 1.2.3-dev",
        "other release",
    ] {
        assert_eq!(parse_version(raw), None);
    }
}

#[test]
fn exact_frame_limit_is_allowed_without_truncation() {
    let mut child = cat();
    let message = b"{\"method\":\"future/ok\"}";
    let mut io = Stdio {
        owner: &mut child,
        frames: Frames::default(),
        rejected: None,
    };
    io.frames.partial = vec![b' '; MAX_FRAME_BYTES - message.len()];
    let mut bytes = message.to_vec();
    bytes.push(b'\n');
    io.send(&bytes, Instant::now() + Duration::from_secs(2))
        .unwrap();
    assert_eq!(
        io.receive(Instant::now() + Duration::from_secs(2))
            .unwrap()
            .unwrap()
            .method(),
        Some("future/ok")
    );
    child.shutdown().unwrap();
}
