//! The parser helper as a real process: its protocol, what it refuses, and the isolation it puts
//! on itself (each layer observed from the inside by the probe, with a baseline that shows the
//! forbidden thing is possible without the layer).

use std::{
    io::{Read, Write},
    os::unix::process::ExitStatusExt,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};
use wyss_console::screen::{
    Limits, Terminal,
    ipc::{self, IpcError, MAX_REQUEST, MAX_RESPONSE, PROTOCOL, Refusal, Request, Response, layer},
    sandbox,
};

const HELPER: &str = env!("CARGO_BIN_EXE_wyss-screen");

struct Helper {
    child: Child,
    to: ChildStdin,
    from: ChildStdout,
}

fn spawn(args: &[&str]) -> Helper {
    let mut child = Command::new(HELPER)
        .args(args)
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the helper starts");
    let to = child.stdin.take().unwrap();
    let from = child.stdout.take().unwrap();
    Helper { child, to, from }
}

impl Helper {
    fn send(&mut self, r: &Request) {
        ipc::write_frame(&mut self.to, &r.encode(), MAX_REQUEST).unwrap();
    }
    fn raw(&mut self, bytes: &[u8]) {
        self.to.write_all(bytes).unwrap();
        self.to.flush().unwrap();
    }
    fn recv(&mut self) -> Result<Response, IpcError> {
        match ipc::read_frame(&mut self.from, MAX_RESPONSE)? {
            Some(f) => Response::decode(&f),
            None => Err(IpcError::Io(std::io::ErrorKind::UnexpectedEof)),
        }
    }
    fn hello(&mut self, rows: u16, cols: u16) -> Response {
        self.send(&Request::Hello {
            version: PROTOCOL,
            rows,
            cols,
            scrollback_lines: 2000,
            width_table: 0,
            require: layer::ALL,
        });
        self.recv().unwrap()
    }
    fn finish(mut self) -> std::process::ExitStatus {
        drop(self.to);
        let mut rest = Vec::new();
        let _ = self.from.read_to_end(&mut rest);
        self.child.wait().unwrap()
    }
}

fn landlock_there() -> bool {
    sandbox::landlock_abi() > 0
}

#[test]
fn a_helper_serves_a_screen_and_its_repaint_redraws_it() {
    let mut h = spawn(&[]);
    let Response::Ready {
        applied,
        pid,
        version,
        ..
    } = h.hello(10, 40)
    else {
        panic!()
    };
    assert_eq!(version, PROTOCOL);
    assert_eq!(pid, h.child.id());
    assert_eq!(applied, layer::ALL, "every layer in force on this machine");
    let stream = b"\x1b[2J\x1b[Hhello \x1b[1;31mworld\x1b[0m\r\n\xe4\xb8\x96\xe7\x95\x8c second\x1b[?1000h\x1b[?25l";
    let mut offset = 100u64;
    for piece in stream.chunks(7) {
        h.send(&Request::Feed {
            offset,
            bytes: piece.to_vec(),
        });
        offset += piece.len() as u64;
        let Response::Fed { end, .. } = h.recv().unwrap() else {
            panic!()
        };
        assert_eq!(end, offset);
    }
    h.send(&Request::Snapshot {
        scrollback_lines: 100,
    });
    let Response::Snapshot(snap) = h.recv().unwrap() else {
        panic!()
    };
    assert_eq!((snap.rows, snap.cols), (10, 40));
    assert_eq!(snap.fed, stream.len() as u64);
    let mut local = Terminal::new(10, 40, Limits::default());
    local.feed(&snap.repaint);
    let mut direct = Terminal::new(10, 40, Limits::default());
    direct.feed(stream);
    assert_eq!(local.text(), direct.text());
    assert_eq!(local.modes(), direct.modes());
    assert_eq!(snap.modes, direct.modes());
    h.send(&Request::Ping { nonce: 77 });
    assert_eq!(h.recv().unwrap(), Response::Pong { nonce: 77 });
    h.send(&Request::Resize { rows: 5, cols: 20 });
    let Response::Fed { generation, .. } = h.recv().unwrap() else {
        panic!()
    };
    assert!(generation >= 1);
    h.send(&Request::Shutdown);
    assert_eq!(h.finish().code(), Some(0));
}

#[test]
fn a_closed_input_is_a_clean_end_and_a_cut_frame_is_not() {
    let mut h = spawn(&[]);
    h.hello(4, 10);
    assert_eq!(h.finish().code(), Some(0));
    let mut h = spawn(&[]);
    h.hello(4, 10);
    h.raw(&[0, 0, 0, 9, 2]);
    assert_eq!(h.finish().code(), Some(3));
}

#[test]
fn what_it_does_not_understand_ends_it_with_a_closed_word() {
    let cases: Vec<(&str, Vec<u8>, Refusal)> = vec![
        (
            "not a hello first",
            Request::Ping { nonce: 1 }.encode(),
            Refusal::NotReady,
        ),
        ("unknown kind", vec![0x7f], Refusal::NotReady),
        (
            "wrong version",
            Request::Hello {
                version: 9,
                rows: 4,
                cols: 4,
                scrollback_lines: 1,
                width_table: 0,
                require: 0,
            }
            .encode(),
            Refusal::Version,
        ),
        (
            "zero rows",
            Request::Hello {
                version: PROTOCOL,
                rows: 0,
                cols: 4,
                scrollback_lines: 1,
                width_table: 0,
                require: 0,
            }
            .encode(),
            Refusal::Size,
        ),
        (
            "too many columns",
            Request::Hello {
                version: PROTOCOL,
                rows: 4,
                cols: 513,
                scrollback_lines: 1,
                width_table: 0,
                require: 0,
            }
            .encode(),
            Refusal::Size,
        ),
        (
            "history beyond the bound",
            Request::Hello {
                version: PROTOCOL,
                rows: 4,
                cols: 4,
                scrollback_lines: 1_000_000,
                width_table: 0,
                require: 0,
            }
            .encode(),
            Refusal::Size,
        ),
    ];
    for (what, frame, expected) in cases {
        let mut h = spawn(&[]);
        ipc::write_frame(&mut h.to, &frame, MAX_REQUEST).unwrap();
        assert_eq!(
            h.recv().unwrap(),
            Response::Refused { code: expected },
            "{what}"
        );
        assert_eq!(h.finish().code(), Some(3), "{what}");
    }
}

#[test]
fn after_hello_a_second_hello_a_gap_in_the_stream_and_a_bad_size_are_refused() {
    let hello = Request::Hello {
        version: PROTOCOL,
        rows: 4,
        cols: 4,
        scrollback_lines: 1,
        width_table: 0,
        require: 0,
    };
    for (what, bad, expected) in [
        ("hello again", hello.clone(), Refusal::AlreadyReady),
        (
            "resize to nothing",
            Request::Resize { rows: 0, cols: 4 },
            Refusal::Size,
        ),
        (
            "resize beyond",
            Request::Resize { rows: 4, cols: 600 },
            Refusal::Size,
        ),
    ] {
        let mut h = spawn(&[]);
        h.send(&hello);
        h.recv().unwrap();
        h.send(&bad);
        assert_eq!(
            h.recv().unwrap(),
            Response::Refused { code: expected },
            "{what}"
        );
        assert_eq!(h.finish().code(), Some(3), "{what}");
    }
    let mut h = spawn(&[]);
    h.send(&hello);
    h.recv().unwrap();
    h.send(&Request::Feed {
        offset: 0,
        bytes: b"abc".to_vec(),
    });
    h.recv().unwrap();
    h.send(&Request::Feed {
        offset: 99,
        bytes: b"x".to_vec(),
    });
    assert_eq!(
        h.recv().unwrap(),
        Response::Refused {
            code: Refusal::OutOfOrder
        }
    );
    assert_eq!(h.finish().code(), Some(3));
}

#[test]
fn an_oversized_or_empty_frame_is_refused_before_it_is_read() {
    for header in [
        (MAX_REQUEST as u32 + 1).to_be_bytes(),
        [0, 0, 0, 0],
        u32::MAX.to_be_bytes(),
    ] {
        let mut h = spawn(&[]);
        h.raw(&header);
        assert_eq!(
            h.recv().unwrap(),
            Response::Refused {
                code: Refusal::BadFrame
            }
        );
        assert_eq!(h.finish().code(), Some(3));
    }
}

#[test]
fn a_required_layer_that_is_not_a_layer_is_a_usage_error() {
    for args in [
        vec!["--require", "16"],
        vec!["--require", "x"],
        vec!["--nonsense"],
        vec!["--require"],
    ] {
        let status = Command::new(HELPER)
            .args(&args)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn a_helper_that_was_required_to_be_sandboxed_says_what_it_is() {
    // Requiring nothing still reports what was applied: the host decides whether that is enough.
    let mut h = spawn(&["--require", "0"]);
    let Response::Ready { applied, .. } = h.hello(4, 4) else {
        panic!()
    };
    assert_eq!(applied & layer::NO_NEW_PRIVS, layer::NO_NEW_PRIVS);
    h.send(&Request::Shutdown);
    h.finish();
}

// ---- the isolation, observed from inside ----------------------------------------------------

fn probe(layers: u8, action: &str) -> (Option<i32>, Option<i32>, String) {
    let out = Command::new(HELPER)
        .args(["--sandbox-probe", &layers.to_string(), action])
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .unwrap();
    (
        out.status.code(),
        out.status.signal(),
        String::from_utf8_lossy(&out.stdout).trim().to_owned(),
    )
}

const EACCES: &str = "errno=13";
const SIGSYS: i32 = 31;

#[test]
fn landlock_denies_every_file_access_and_without_it_they_work() {
    if !landlock_there() {
        eprintln!("NOT_RUN: this kernel has no Landlock");
        return;
    }
    for action in ["open_passwd", "read_proc_environ"] {
        assert_eq!(probe(0, action).2, "opened", "baseline {action}");
        assert_eq!(
            probe(layer::NO_NEW_PRIVS | layer::LANDLOCK, action).2,
            EACCES,
            "{action}"
        );
    }
    assert_eq!(probe(0, "create_file").2, "created", "baseline");
    assert_eq!(
        probe(layer::NO_NEW_PRIVS | layer::LANDLOCK, "create_file").2,
        EACCES
    );
}

#[test]
fn landlock_denies_tcp_and_signalling_outside_its_domain_where_the_kernel_can() {
    let abi = sandbox::landlock_abi();
    let both = layer::NO_NEW_PRIVS | layer::LANDLOCK;
    if abi >= 4 {
        let base = probe(0, "tcp_connect").2;
        assert!(
            base.contains("errno=111"),
            "baseline reaches the network stack: {base}"
        );
        assert!(
            probe(both, "tcp_connect").2.contains("errno=13"),
            "{}",
            probe(both, "tcp_connect").2
        );
    } else {
        eprintln!("NOT_RUN: Landlock ABI {abi} has no network rules");
    }
    if abi >= 6 {
        assert!(probe(0, "signal_parent").2.contains("kill=0"));
        assert!(
            probe(both, "signal_parent").2.contains("errno=1"),
            "{}",
            probe(both, "signal_parent").2
        );
    } else {
        eprintln!("NOT_RUN: Landlock ABI {abi} has no signal scope");
    }
}

#[test]
fn seccomp_ends_the_process_that_asks_for_a_socket_a_child_a_program_or_a_trace() {
    let armed = layer::NO_NEW_PRIVS | layer::SECCOMP;
    assert_eq!(probe(0, "unix_socket").2, "created", "baseline");
    assert_eq!(probe(0, "fork").2, "forked", "baseline");
    for action in ["unix_socket", "tcp_connect", "fork", "exec", "ptrace"] {
        let (code, signal, out) = probe(armed, action);
        assert_eq!((signal, code), (Some(SIGSYS), None), "{action}: {out}");
    }
    // What the parser needs still works under the filter.
    assert!(probe(armed, "applied").2.starts_with("layers="));
}

#[test]
fn the_resource_ceilings_hold() {
    assert_eq!(probe(0, "allocate_gib").2, "mapped", "baseline");
    assert_eq!(
        probe(layer::NO_NEW_PRIVS | layer::RLIMITS, "allocate_gib").2,
        "errno=12"
    );
    assert_eq!(probe(0, "descriptors").2, "dups=64", "baseline");
    let capped = probe(layer::NO_NEW_PRIVS | layer::RLIMITS, "descriptors").2;
    let n: u32 = capped.trim_start_matches("dups=").parse().unwrap();
    assert!(n < 8, "{capped}");
}

#[test]
fn all_layers_together_leave_a_process_that_can_still_do_its_work() {
    let (code, signal, out) = probe(layer::ALL, "applied");
    assert_eq!((code, signal), (Some(0), None), "{out}");
    let bits: u8 = out
        .split(' ')
        .next()
        .unwrap()
        .trim_start_matches("layers=")
        .parse()
        .unwrap();
    assert_eq!(bits, layer::ALL);
    for action in ["open_passwd", "create_file", "allocate_gib"] {
        let (_, signal, out) = probe(layer::ALL, action);
        assert!(
            signal.is_some() || out.starts_with("errno="),
            "{action}: {out}"
        );
    }
}

// ---- hostile and heavy input --------------------------------------------------------------

#[test]
fn hostile_output_is_counted_and_a_large_flood_stays_within_its_memory_bound() {
    let mut h = spawn(&[]);
    h.hello(24, 80);
    let hostile = b"\x1b]52;c;QUJD\x07\x1b]8;;http://x.invalid\x07\x1b[8;1;1t\x1b[c\x1bP1;2q\x1b\\";
    h.send(&Request::Feed {
        offset: 0,
        bytes: hostile.to_vec(),
    });
    h.recv().unwrap();
    // 8 MiB of mixed text and escapes, fed in the largest frames the protocol allows.
    let unit = b"line of text \x1b[1;32mgreen\x1b[0m \xe4\xb8\x96 \x1b[5;10H\x1b[K\x1b[2J\r\n";
    let chunk: Vec<u8> = unit.iter().copied().cycle().take(ipc::MAX_FEED).collect();
    let mut offset = hostile.len() as u64;
    let started = std::time::Instant::now();
    for _ in 0..128 {
        h.send(&Request::Feed {
            offset,
            bytes: chunk.clone(),
        });
        let Response::Fed { end, .. } = h.recv().unwrap() else {
            panic!()
        };
        offset = end;
    }
    let elapsed = started.elapsed();
    let status = std::fs::read_to_string(format!("/proc/{}/status", h.child.id())).unwrap();
    let kib = |key: &str| -> u64 {
        status
            .lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap()
            .parse()
            .unwrap()
    };
    let peak = kib("VmHWM:");
    assert!(peak < 96 * 1024, "peak resident {peak} KiB");
    h.send(&Request::Snapshot {
        scrollback_lines: 2000,
    });
    let Response::Snapshot(s) = h.recv().unwrap() else {
        panic!()
    };
    assert_eq!((s.counters.clipboard, s.counters.hyperlink), (1, 1));
    assert!(s.counters.window_op >= 1 && s.counters.query >= 1 && s.counters.string >= 1);
    assert!(s.repaint.len() <= wyss_console::screen::repaint::MAX_REPAINT);
    eprintln!("flood: 8 MiB in {elapsed:?}, peak {peak} KiB");
    h.send(&Request::Shutdown);
    h.finish();
}
