use super::*;
use crate::contracts::WireUuid;
use nix::{errno::Errno, sys::signal::kill, unistd::Pid};

fn request() -> CreateTerminal {
    CreateTerminal {
        size: TerminalSize { rows: 24, cols: 80 },
        cwd: Some(env!("CARGO_MANIFEST_DIR").into()),
    }
}
fn shell(service: &TerminalService) -> TerminalInfo {
    service
        .create_with_shell(request(), PathBuf::from("/bin/sh"), PathBuf::from("/"))
        .unwrap()
}
fn wait_for(service: &TerminalService, id: &TerminalId, predicate: impl Fn(&LocalState) -> bool) {
    let runtime = service.runtime(id).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut state = lock(&runtime.shared.data);
    while !predicate(&state) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "timed out: {:?}", state.info);
        state = runtime
            .shared
            .changed
            .wait_timeout(state, remaining)
            .unwrap()
            .0;
    }
}
fn output_until(service: &TerminalService, id: &TerminalId, expected: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut ack: Option<WireUuid> = None;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(
            Instant::now() < deadline,
            "output not found: {:?}",
            String::from_utf8_lossy(&output)
        );
        let batch = service.read(id, ack.as_ref()).unwrap();
        ack = batch.batch_id;
        output.extend(batch.data);
        assert!(output.len() <= OUTPUT_CAPACITY, "test capture limit");
        if output.windows(expected.len()).any(|part| part == expected) {
            return output;
        }
        let runtime = service.runtime(id).unwrap();
        let state = lock(&runtime.shared.data);
        if state.output.len() == 0 && !state.completed {
            drop(
                runtime
                    .shared
                    .changed
                    .wait_timeout(state, deadline.saturating_duration_since(Instant::now()))
                    .unwrap(),
            );
        }
    }
}

#[test]
fn creates_real_pty_with_unique_identity_and_valid_cwd() {
    let service = TerminalService::default();
    let terminal = shell(&service);
    assert_eq!(terminal.state, TerminalState::Running);
    assert!(terminal.observed_pid.is_some());
    assert_eq!(
        terminal.cwd,
        std::fs::canonicalize(env!("CARGO_MANIFEST_DIR"))
            .unwrap()
            .to_string_lossy()
    );
    service
        .write(
            &terminal.terminal_id,
            b"test -t 0 && printf '\\nPTY_REAL\\n'; pwd\n".to_vec(),
        )
        .unwrap();
    let bytes = output_until(&service, &terminal.terminal_id, b"\r\nPTY_REAL\r\n");
    assert!(!bytes.is_empty());
    service.close(&terminal.terminal_id).unwrap();
}

#[test]
fn two_terminals_interact_independently_and_accept_multiple_writes() {
    let service = TerminalService::default();
    let first = shell(&service);
    let second = shell(&service);
    assert_ne!(first.terminal_id, second.terminal_id);
    assert_ne!(first.observed_pid, second.observed_pid);
    service
        .write(&first.terminal_id, b"printf '\\nONE_".to_vec())
        .unwrap();
    service
        .write(&first.terminal_id, b"ONLY\\n'\n".to_vec())
        .unwrap();
    service
        .write(&second.terminal_id, b"printf '\\nTWO_ONLY\\n'\n".to_vec())
        .unwrap();
    let first_output = output_until(&service, &first.terminal_id, b"\r\nONE_ONLY\r\n");
    let second_output = output_until(&service, &second.terminal_id, b"\r\nTWO_ONLY\r\n");
    assert!(!first_output.windows(8).any(|value| value == b"TWO_ONLY"));
    assert!(!second_output.windows(8).any(|value| value == b"ONE_ONLY"));
    service.shutdown().unwrap();
    assert!(lock(&service.registry.active).is_empty());
}

#[test]
fn byte_output_is_not_decoded_or_ansi_parsed() {
    let service = TerminalService::default();
    let terminal = shell(&service);
    service
        .write(
            &terminal.terminal_id,
            b"printf '\\342'; printf '\\202\\254\\033[31m\\377'\n".to_vec(),
        )
        .unwrap();
    let bytes = output_until(&service, &terminal.terminal_id, b"\xe2\x82\xac\x1b[31m\xff");
    assert!(bytes.contains(&255));
    service.close(&terminal.terminal_id).unwrap();
}

#[test]
fn resize_reaches_real_pty_and_shell() {
    let service = TerminalService::default();
    let terminal = shell(&service);
    let info = service
        .resize(
            &terminal.terminal_id,
            TerminalSize {
                rows: 37,
                cols: 111,
            },
        )
        .unwrap();
    assert_eq!(info.size.rows, 37);
    service
        .write(&terminal.terminal_id, b"stty size\n".to_vec())
        .unwrap();
    output_until(&service, &terminal.terminal_id, b"37 111\r\n");
    service.close(&terminal.terminal_id).unwrap();
}

#[test]
fn invalid_dimensions_reject_create_and_resize() {
    let service = TerminalService::default();
    let terminal = shell(&service);
    for size in [
        TerminalSize { rows: 0, cols: 80 },
        TerminalSize { rows: 24, cols: 0 },
        TerminalSize {
            rows: 513,
            cols: 80,
        },
        TerminalSize {
            rows: 24,
            cols: 513,
        },
    ] {
        assert_eq!(
            service
                .create(CreateTerminal { size, cwd: None })
                .unwrap_err()
                .code,
            ErrorCode::InvalidDimensions
        );
        assert_eq!(
            service
                .resize(&terminal.terminal_id, size)
                .unwrap_err()
                .code,
            ErrorCode::InvalidDimensions
        );
    }
    service.close(&terminal.terminal_id).unwrap();
}

#[test]
fn invalid_cwd_never_falls_back() {
    let service = TerminalService::default();
    for cwd in [
        format!("{}/does-not-exist", env!("CARGO_MANIFEST_DIR")),
        format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR")),
    ] {
        assert_eq!(
            service
                .create(CreateTerminal {
                    cwd: Some(cwd),
                    ..request()
                })
                .unwrap_err()
                .code,
            ErrorCode::InvalidCwd
        );
    }
    assert!(lock(&service.registry.active).is_empty());
}

#[test]
fn unknown_id_operations_reject_deterministically() {
    let service = TerminalService::default();
    let unknown = TerminalId::parse(&uuid::Uuid::new_v4().to_string()).unwrap();
    assert_eq!(
        service.write(&unknown, b"x".to_vec()).unwrap_err().code,
        ErrorCode::UnknownTerminal
    );
    assert_eq!(
        service.resize(&unknown, request().size).unwrap_err().code,
        ErrorCode::UnknownTerminal
    );
    assert_eq!(
        service.close(&unknown).unwrap_err().code,
        ErrorCode::UnknownTerminal
    );
    assert_eq!(
        service.read(&unknown, None).unwrap_err().code,
        ErrorCode::UnknownTerminal
    );
}

#[test]
fn natural_exit_is_reaped_and_remaining_output_is_readable() {
    let service = TerminalService::default();
    let terminal = shell(&service);
    service
        .write(
            &terminal.terminal_id,
            b"printf '\\nEXIT_MARKER\\n'; exit 7\n".to_vec(),
        )
        .unwrap();
    wait_for(&service, &terminal.terminal_id, |state| state.completed);
    let info = service.status(&terminal.terminal_id).unwrap();
    assert_eq!(info.state, TerminalState::Exited);
    assert_eq!(info.exit.unwrap().code, 7);
    assert_eq!(
        kill(Pid::from_raw(terminal.observed_pid.unwrap() as i32), None),
        Err(Errno::ESRCH)
    );
    output_until(&service, &terminal.terminal_id, b"\r\nEXIT_MARKER\r\n");
    assert_eq!(
        service
            .write(&terminal.terminal_id, b"x".to_vec())
            .unwrap_err()
            .code,
        ErrorCode::AlreadyClosed
    );
    service.close(&terminal.terminal_id).unwrap();
}

#[test]
fn close_releases_handles_reaps_child_and_removes_registry_entry() {
    let service = TerminalService::default();
    let terminal = shell(&service);
    let runtime = service.runtime(&terminal.terminal_id).unwrap();
    let info = service.close(&terminal.terminal_id).unwrap();
    assert_eq!(info.state, TerminalState::Closed);
    assert!(info.exit.is_some());
    assert!(lock(&runtime.worker).is_none());
    assert!(lock(&runtime.shared.data).completed);
    assert!(lock(&service.registry.active).is_empty());
    assert_eq!(
        kill(Pid::from_raw(terminal.observed_pid.unwrap() as i32), None),
        Err(Errno::ESRCH)
    );
    assert_eq!(
        service.close(&terminal.terminal_id).unwrap_err().code,
        ErrorCode::AlreadyClosed
    );
    assert_eq!(
        service
            .resize(&terminal.terminal_id, request().size)
            .unwrap_err()
            .code,
        ErrorCode::AlreadyClosed
    );
    assert_eq!(
        service
            .write(&terminal.terminal_id, b"x".to_vec())
            .unwrap_err()
            .code,
        ErrorCode::AlreadyClosed
    );
    assert_eq!(
        service.status(&terminal.terminal_id).unwrap().state,
        TerminalState::Closed
    );
}

#[test]
fn close_escalates_when_shell_ignores_hangup() {
    let service = TerminalService::default();
    let terminal = shell(&service);
    service
        .write(
            &terminal.terminal_id,
            b"trap '' HUP TERM; printf '\\nIGNORE_READY\\n'; sleep 30\n".to_vec(),
        )
        .unwrap();
    output_until(&service, &terminal.terminal_id, b"\r\nIGNORE_READY\r\n");
    let began = Instant::now();
    let closed = service.close(&terminal.terminal_id).unwrap();
    assert!(began.elapsed() < CLOSE_WAIT);
    assert!(closed.exit.unwrap().signal.is_some());
}

#[test]
fn nonreading_consumer_fills_bounded_queue_but_close_still_completes() {
    let service = TerminalService::default();
    let terminal = shell(&service);
    service
        .write(&terminal.terminal_id, b"yes WYSS_BOUNDED_OUTPUT\n".to_vec())
        .unwrap();
    wait_for(&service, &terminal.terminal_id, |state| {
        state.output.len() == OUTPUT_CAPACITY
    });
    let runtime = service.runtime(&terminal.terminal_id).unwrap();
    assert_eq!(lock(&runtime.shared.data).output.len(), OUTPUT_CAPACITY);
    let first = service.read(&terminal.terminal_id, None).unwrap();
    assert_eq!(first.data.len(), OUTPUT_BATCH);
    assert_eq!(
        service.read(&terminal.terminal_id, None).unwrap_err().code,
        ErrorCode::InvalidAck
    );
    wait_for(&service, &terminal.terminal_id, |state| {
        state.output.len() == OUTPUT_CAPACITY
    });
    let info = service.close(&terminal.terminal_id).unwrap();
    assert_eq!(info.state, TerminalState::Closed);
    assert_eq!(lock(&runtime.shared.data).output.len(), 0);
}

#[test]
fn bounded_buffer_preserves_split_binary_chunks_and_credit() {
    let mut buffer = OutputBuffer::new();
    for part in [
        &[0xe2][..],
        &[0x82, 0xac, 0x1b][..],
        &[b'[', b'3', b'1', b'm', 0xff][..],
    ] {
        buffer.append(part);
    }
    let (ack, bytes) = buffer.take(None).unwrap();
    assert_eq!(bytes, b"\xe2\x82\xac\x1b[31m\xff");
    assert_eq!(buffer.take(None).unwrap_err().code, ErrorCode::InvalidAck);
    let (ack, bytes) = buffer.take(ack.as_ref()).unwrap();
    assert!(ack.is_none());
    assert!(bytes.is_empty());
    buffer.append(&vec![42; OUTPUT_CAPACITY]);
    let (ack, first) = buffer.take(None).unwrap();
    assert_eq!(first.len(), OUTPUT_BATCH);
    let (_, second) = buffer.take(ack.as_ref()).unwrap();
    assert_eq!(second.len(), OUTPUT_BATCH);
    assert!(buffer.len() <= OUTPUT_CAPACITY);
}

#[test]
fn invalid_input_and_spawn_failures_are_structured_and_do_not_leave_entries() {
    let service = TerminalService::default();
    let failure = service
        .create_with_shell(
            request(),
            PathBuf::from("/wyss/nonexistent-shell"),
            PathBuf::from("/"),
        )
        .unwrap_err();
    assert_eq!(failure.code, ErrorCode::SpawnFailed);
    assert!(failure.diagnostic.is_some());
    let wire = serde_json::to_value(failure).unwrap();
    assert!(wire.get("diagnostic").is_none());
    assert!(lock(&service.registry.active).is_empty());
    let terminal = shell(&service);
    for bytes in [vec![], vec![b'x'; INPUT_LIMIT + 1]] {
        assert_eq!(
            service
                .write(&terminal.terminal_id, bytes)
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput
        );
    }
    service.close(&terminal.terminal_id).unwrap();
}

#[test]
fn production_shell_selection_uses_account_shell_and_default_home() {
    let service = TerminalService::default();
    let (expected_shell, home) = shell_and_home().unwrap();
    let terminal = service
        .create(CreateTerminal {
            cwd: None,
            ..request()
        })
        .unwrap();
    assert_eq!(terminal.shell, expected_shell.to_string_lossy());
    assert_eq!(terminal.cwd, home.canonicalize().unwrap().to_string_lossy());
    // With xterm-256color fish legitimately waits for a renderer handshake.
    // This backend-only regression checks real account-shell spawn/cleanup;
    // interaction and capability replies are validated with the real GUI renderer.
    let pid = Pid::from_raw(terminal.observed_pid.unwrap() as i32);
    assert_eq!(kill(pid, None), Ok(()));
    service.close(&terminal.terminal_id).unwrap();
    assert_eq!(kill(pid, None), Err(Errno::ESRCH));
}

#[test]
fn command_queue_is_bounded() {
    let (sender, _receiver) = mpsc::sync_channel(COMMAND_CAPACITY);
    for _ in 0..COMMAND_CAPACITY {
        let (reply, _result) = mpsc::sync_channel(1);
        sender
            .try_send(Command::Write {
                data: vec![0; INPUT_LIMIT],
                deadline: Instant::now(),
                reply,
            })
            .unwrap();
    }
    let (reply, _) = mpsc::sync_channel(1);
    assert!(matches!(
        sender.try_send(Command::Write {
            data: vec![1],
            deadline: Instant::now(),
            reply
        }),
        Err(mpsc::TrySendError::Full(_))
    ));
}

#[test]
fn create_dto_cannot_select_an_executable() {
    let value = serde_json::json!({ "size": { "rows": 24, "cols": 80 }, "cwd": null, "executable": "/bin/anything" });
    assert!(serde_json::from_value::<CreateTerminal>(value).is_err());
}

#[test]
fn input_backpressure_times_out_with_partial_write_diagnostics() {
    let service = TerminalService::default();
    let terminal = shell(&service);
    service
        .write(
            &terminal.terminal_id,
            b"stty -echo -icanon; printf '\\nNO_INPUT_READER\\n'; sleep 30\n".to_vec(),
        )
        .unwrap();
    output_until(&service, &terminal.terminal_id, b"\r\nNO_INPUT_READER\r\n");
    let mut rejected = None;
    for _ in 0..32 {
        if let Err(error) = service.write(&terminal.terminal_id, vec![b'x'; INPUT_LIMIT]) {
            rejected = Some(error);
            break;
        }
    }
    let error = rejected.expect("PTY input must eventually apply bounded backpressure");
    assert_eq!(error.code, ErrorCode::WriteFailed);
    assert!(error.bytes_written <= INPUT_LIMIT);
    service.close(&terminal.terminal_id).unwrap();
}

#[test]
fn concurrent_close_has_exactly_one_success() {
    let service = TerminalService::default();
    let terminal = shell(&service);
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let mut callers = Vec::new();
    for _ in 0..2 {
        let service = service.clone();
        let id = terminal.terminal_id.clone();
        let barrier = barrier.clone();
        callers.push(thread::spawn(move || {
            barrier.wait();
            service.close(&id)
        }));
    }
    barrier.wait();
    let results: Vec<_> = callers
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter_map(|result| result.as_ref().err())
            .next()
            .unwrap()
            .code,
        ErrorCode::AlreadyClosed
    );
}

#[test]
fn registry_capacity_is_bounded_and_reusable_after_close() {
    let service = TerminalService::default();
    let mut terminals = Vec::new();
    for _ in 0..MAX_TERMINALS {
        terminals.push(shell(&service));
    }
    assert_eq!(
        service
            .create_with_shell(request(), PathBuf::from("/bin/sh"), PathBuf::from("/"))
            .unwrap_err()
            .code,
        ErrorCode::LimitReached
    );
    service.close(&terminals[0].terminal_id).unwrap();
    let replacement = shell(&service);
    assert_ne!(replacement.terminal_id, terminals[0].terminal_id);
    service.shutdown().unwrap();
}

#[test]
fn late_completed_close_reclaims_slot_and_keeps_closed_status() {
    let service = TerminalService::default();
    let terminal = shell(&service);
    let runtime = service.runtime(&terminal.terminal_id).unwrap();
    runtime.stop();
    runtime.wait(Instant::now() + CLOSE_WAIT).unwrap();
    assert_eq!(lock(&service.registry.active).len(), 1);
    let next = shell(&service);
    assert_eq!(lock(&service.registry.active).len(), 1);
    assert_eq!(
        service.status(&terminal.terminal_id).unwrap().state,
        TerminalState::Closed
    );
    assert_eq!(
        service.close(&terminal.terminal_id).unwrap_err().code,
        ErrorCode::AlreadyClosed
    );
    service.close(&next.terminal_id).unwrap();
}

#[test]
fn dropping_last_service_requests_child_cleanup() {
    let service = TerminalService::default();
    let terminal = shell(&service);
    let runtime = service.runtime(&terminal.terminal_id).unwrap();
    drop(service);
    assert_eq!(
        runtime.wait(Instant::now() + CLOSE_WAIT).unwrap().state,
        TerminalState::Closed
    );
    assert_eq!(
        kill(Pid::from_raw(terminal.observed_pid.unwrap() as i32), None),
        Err(Errno::ESRCH)
    );
}

#[test]
fn renderer_term_is_scoped_to_spawned_shell() {
    let before = std::env::var_os("TERM");
    let service = TerminalService::default();
    let terminal = shell(&service);
    service
        .write(
            &terminal.terminal_id,
            b"printf '\\nTERM_CHECK:%s\\n' \"$TERM\"\n".to_vec(),
        )
        .unwrap();
    output_until(
        &service,
        &terminal.terminal_id,
        b"\r\nTERM_CHECK:xterm-256color\r\n",
    );
    assert_eq!(std::env::var_os("TERM"), before);
    service.close(&terminal.terminal_id).unwrap();
}
