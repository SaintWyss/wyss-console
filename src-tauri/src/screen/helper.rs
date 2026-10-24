//! The loop of the parser helper process: one terminal's screen, fed output bytes by the host and
//! asked for snapshots. It owns no PTY, no credential and no authority; its only input is the typed
//! frames of `ipc`, and it fails closed: a frame it does not understand ends it.

use super::{
    ipc::{
        self, Counters, IpcError, MAX_REQUEST, MAX_RESPONSE, PROTOCOL, Refusal, Request, Response,
        SnapshotBody, layer,
    },
    sandbox::Applied,
    terminal::{Limits, MAX_COLS, MAX_ROWS, Terminal},
    width::Table,
};
use std::io::{Read, Write};

/// History lines a helper will keep however many the host asks for.
pub const MAX_SCROLLBACK: u32 = 20_000;

/// Why the loop ended; the process exit code follows from it.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Exit {
    /// The host closed the stream, or asked for the end.
    Clean,
    /// A frame or a request the helper refuses to go on after.
    Protocol,
    /// A required isolation layer was not in force.
    Sandbox,
}

impl Exit {
    pub fn code(self) -> i32 {
        match self {
            Exit::Clean => 0,
            Exit::Protocol => 3,
            Exit::Sandbox => 4,
        }
    }
}

fn send(w: &mut impl Write, r: &Response) -> Result<(), IpcError> {
    ipc::write_frame(w, &r.encode(), MAX_RESPONSE)
}

fn refuse(w: &mut impl Write, code: Refusal) -> Exit {
    let _ = send(w, &Response::Refused { code });
    if code == Refusal::SandboxUnavailable {
        Exit::Sandbox
    } else {
        Exit::Protocol
    }
}

/// Serves one terminal. `applied` is what the process already did to itself (before any frame was
/// read): the host's required layers are checked against it.
pub fn serve(input: &mut impl Read, output: &mut impl Write, applied: Applied) -> Exit {
    let first = match ipc::read_frame(input, MAX_REQUEST) {
        Ok(Some(f)) => f,
        Ok(None) => return Exit::Clean,
        Err(_) => return refuse(output, Refusal::BadFrame),
    };
    let Ok(Request::Hello {
        version,
        rows,
        cols,
        scrollback_lines,
        width_table,
        require,
    }) = Request::decode(&first)
    else {
        return refuse(output, Refusal::NotReady);
    };
    if version != PROTOCOL {
        return refuse(output, Refusal::Version);
    }
    if !(1..=MAX_ROWS as u16).contains(&rows)
        || !(1..=MAX_COLS as u16).contains(&cols)
        || scrollback_lines > MAX_SCROLLBACK
        || width_table > 1
    {
        return refuse(output, Refusal::Size);
    }
    if require & !applied.layers != 0 {
        return refuse(output, Refusal::SandboxUnavailable);
    }
    let mut screen = Terminal::new(
        usize::from(rows),
        usize::from(cols),
        Limits {
            width: if width_table == 0 {
                Table::Xterm6
            } else {
                Table::Modern
            },
            scrollback_lines: scrollback_lines as usize,
            ..Limits::default()
        },
    );
    if send(
        output,
        &Response::Ready {
            version: PROTOCOL,
            pid: std::process::id(),
            applied: applied.layers,
            landlock_abi: applied.landlock_abi,
        },
    )
    .is_err()
    {
        return Exit::Protocol;
    }

    // Offsets are the host's own numbering of the terminal's output; the first Feed fixes where
    // this screen starts and every later one must continue exactly where the last ended.
    let mut next: Option<u64> = None;
    loop {
        let frame = match ipc::read_frame(input, MAX_REQUEST) {
            Ok(Some(f)) => f,
            Ok(None) => return Exit::Clean,
            Err(_) => return refuse(output, Refusal::BadFrame),
        };
        let request = match Request::decode(&frame) {
            Ok(r) => r,
            Err(_) => return refuse(output, Refusal::BadFrame),
        };
        let answer = match request {
            Request::Hello { .. } => return refuse(output, Refusal::AlreadyReady),
            Request::Feed { offset, bytes } => {
                if next.is_some_and(|n| n != offset) {
                    return refuse(output, Refusal::OutOfOrder);
                }
                // An offset the stream could never reach is a bad frame, not a wrap-around.
                let Some(end) = offset.checked_add(bytes.len() as u64) else {
                    return refuse(output, Refusal::BadFrame);
                };
                screen.feed(&bytes);
                next = Some(end);
                Response::Fed {
                    end: next.unwrap_or(offset),
                    generation: screen.generation(),
                }
            }
            Request::Resize { rows, cols } => {
                if !(1..=MAX_ROWS as u16).contains(&rows) || !(1..=MAX_COLS as u16).contains(&cols)
                {
                    return refuse(output, Refusal::Size);
                }
                screen.resize(usize::from(rows), usize::from(cols));
                Response::Fed {
                    end: next.unwrap_or(0),
                    generation: screen.generation(),
                }
            }
            Request::Snapshot { scrollback_lines } => {
                let Ok(repaint) = screen.repaint(scrollback_lines.min(MAX_SCROLLBACK) as usize)
                else {
                    // A screen that cannot be drawn in the bound is refused, not cut, and the
                    // helper (and its screen) go on.
                    if send(output, &Response::TooLarge).is_err() {
                        return Exit::Protocol;
                    }
                    continue;
                };
                let cursor = screen.cursor();
                let r = screen.refusals();
                Response::Snapshot(Box::new(SnapshotBody {
                    fed: screen.bytes_fed(),
                    generation: screen.generation(),
                    rows: screen.rows() as u16,
                    cols: screen.cols() as u16,
                    alt_active: screen.alt_active(),
                    modes: screen.modes(),
                    cursor_row: cursor.row as u16,
                    cursor_col: cursor.col as u16,
                    title: screen.title().to_owned(),
                    counters: Counters {
                        clipboard: r.clipboard,
                        hyperlink: r.hyperlink,
                        window_op: r.window_op,
                        query: r.query,
                        oversized: r.oversized,
                        string: r.string,
                        invalid_utf8: r.invalid_utf8,
                    },
                    history_evicted: screen.history().lines_evicted,
                    skipped: screen.history().bytes_skipped,
                    scrollback_included: repaint.scrollback_lines as u32,
                    scrollback_omitted: repaint.scrollback_omitted as u32,
                    repaint: repaint.bytes,
                }))
            }
            Request::Skip { to } => {
                // Only forwards, and only after the stream began.
                let Some(at) = next else {
                    return refuse(output, Refusal::OutOfOrder);
                };
                if to < at {
                    return refuse(output, Refusal::OutOfOrder);
                }
                screen.skip(to - at);
                next = Some(to);
                Response::Fed {
                    end: to,
                    generation: screen.generation(),
                }
            }
            Request::Ping { nonce } => Response::Pong { nonce },
            Request::Shutdown => return Exit::Clean,
        };
        if send(output, &answer).is_err() {
            return Exit::Protocol;
        }
    }
}

/// Layers named by `--require`, or all of them.
pub fn parse_require(args: &[String]) -> Option<u8> {
    match args {
        [] => Some(layer::ALL),
        [flag, value] if flag == "--require" => {
            value.parse::<u8>().ok().filter(|v| v & !layer::ALL == 0)
        }
        _ => None,
    }
}

/// Isolation probe: applies the layers named and then tries one thing a parser must not be able
/// to do, printing what the kernel answered. Only the sandbox tests use it.
pub fn probe(layers: u8, action: &str) -> String {
    use nix::libc;
    let applied = match super::sandbox::apply(layers, 0) {
        Ok(a) => a,
        Err(_) => return "apply_failed".to_owned(),
    };
    let errno = || std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
    let cstr = |s: &str| std::ffi::CString::new(s).unwrap();
    match action {
        "applied" => format!("layers={} abi={}", applied.layers, applied.landlock_abi),
        #[cfg(target_arch = "x86_64")]
        "x32_socket" => {
            // No pointer arguments: exercise the alternate syscall ABI using the real filter.
            let fd = unsafe {
                libc::syscall(
                    0x4000_0000 | libc::SYS_socket,
                    libc::AF_INET,
                    libc::SOCK_STREAM,
                    0,
                )
            };
            if fd >= 0 {
                unsafe { libc::close(fd as i32) };
                "created".to_owned()
            } else {
                format!("errno={}", errno())
            }
        }
        "open_passwd" => {
            let fd = unsafe { libc::open(cstr("/etc/passwd").as_ptr(), libc::O_RDONLY) };
            if fd >= 0 {
                "opened".to_owned()
            } else {
                format!("errno={}", errno())
            }
        }
        "create_file" => {
            let fd = unsafe {
                libc::open(
                    cstr("/tmp/wyss-screen-probe").as_ptr(),
                    libc::O_WRONLY | libc::O_CREAT,
                    0o600,
                )
            };
            if fd >= 0 {
                unsafe { libc::unlink(cstr("/tmp/wyss-screen-probe").as_ptr()) };
                "created".to_owned()
            } else {
                format!("errno={}", errno())
            }
        }
        "read_proc_environ" => {
            let fd = unsafe { libc::open(cstr("/proc/self/environ").as_ptr(), libc::O_RDONLY) };
            if fd >= 0 {
                "opened".to_owned()
            } else {
                format!("errno={}", errno())
            }
        }
        "tcp_connect" => {
            let s = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
            if s < 0 {
                return format!("socket errno={}", errno());
            }
            let addr = libc::sockaddr_in {
                sin_family: libc::AF_INET as u16,
                sin_port: 9u16.to_be(),
                sin_addr: libc::in_addr {
                    s_addr: u32::from_be_bytes([127, 0, 0, 1]).to_be(),
                },
                sin_zero: [0; 8],
            };
            let r = unsafe {
                libc::connect(
                    s,
                    &addr as *const _ as *const libc::sockaddr,
                    std::mem::size_of::<libc::sockaddr_in>() as u32,
                )
            };
            format!("connect={} errno={}", r, errno())
        }
        "unix_socket" => {
            let s = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
            if s >= 0 {
                "created".to_owned()
            } else {
                format!("errno={}", errno())
            }
        }
        "fork" => {
            let pid = unsafe { libc::fork() };
            if pid == 0 {
                unsafe { libc::_exit(0) };
            }
            if pid > 0 {
                "forked".to_owned()
            } else {
                format!("errno={}", errno())
            }
        }
        "exec" => {
            let program = cstr("/usr/bin/true");
            let argv = [program.as_ptr(), std::ptr::null()];
            unsafe { libc::execv(program.as_ptr(), argv.as_ptr()) };
            format!("errno={}", errno())
        }
        "ptrace" => {
            let r = unsafe { libc::ptrace(libc::PTRACE_TRACEME, 0, 0, 0) };
            format!("ptrace={} errno={}", r, errno())
        }
        "signal_parent" => {
            let r = unsafe { libc::kill(libc::getppid(), 0) };
            format!("kill={} errno={}", r, errno())
        }
        "allocate_gib" => {
            let p = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    1 << 30,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                    -1,
                    0,
                )
            };
            if p == libc::MAP_FAILED {
                format!("errno={}", errno())
            } else {
                "mapped".to_owned()
            }
        }
        "write_big_file" => "skipped".to_owned(),
        "descriptors" => {
            let mut opened = 0;
            for _ in 0..64 {
                let fd = unsafe { libc::dup(1) };
                if fd < 0 {
                    break;
                }
                opened += 1;
            }
            format!("dups={opened}")
        }
        _ => "unknown_action".to_owned(),
    }
}
