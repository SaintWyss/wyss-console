//! A worker stand-in for the B1 acceptance tests (fixture builds only): it performs one named
//! action with raw system calls and prints one JSON line with what the kernel answered. It is
//! not a tool anyone uses; it is the "entirely untrusted command" of doc 51 §3 made small enough to
//! read, so the tests do not depend on the cooperation of a fake agent: every denial is forced.
//!
//! The attack-shaped actions run only against paths and sockets the tests create for the purpose,
//! with synthetic content; the tests also run each of them *without* confinement, to show that the
//! attack works there (a positive control), so a denial means something.

use nix::libc;
use serde_json::{Value, json};
use std::{
    ffi::CString,
    io::{Read, Write},
    os::unix::{ffi::OsStrExt, net::UnixStream},
    path::Path,
    time::Duration,
};

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn c(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}

fn result(ok: bool, errno: i32, detail: Value) -> Value {
    json!({"ok": ok, "errno": errno, "detail": detail})
}

fn from_io<T>(r: std::io::Result<T>, detail: impl FnOnce(T) -> Value) -> Value {
    match r {
        Ok(v) => result(true, 0, detail(v)),
        Err(e) => result(false, e.raw_os_error().unwrap_or(-1), Value::Null),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a = |i: usize| args.get(i).map_or("", String::as_str);
    let out = match a(0) {
        "read" => from_io(std::fs::read(a(1)), |b| {
            json!(String::from_utf8_lossy(&b[..b.len().min(200)]))
        }),
        "write" => from_io(std::fs::write(a(1), a(2)), |()| Value::Null),
        "append" => from_io(
            std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(a(1))
                .and_then(|mut f| f.write_all(a(2).as_bytes())),
            |()| Value::Null,
        ),
        // Writes into an existing file without replacing it: the attack on a shared inode.
        "overwrite" => from_io(
            std::fs::OpenOptions::new()
                .write(true)
                .open(a(1))
                .and_then(|mut f| f.write_all(a(2).as_bytes())),
            |()| Value::Null,
        ),
        "exists" => result(Path::new(a(1)).exists(), 0, Value::Null),
        "list" => from_io(std::fs::read_dir(a(1)), |d| {
            let mut names: Vec<String> = d
                .filter_map(|e| Some(e.ok()?.file_name().to_string_lossy().into_owned()))
                .collect();
            names.sort();
            json!(names)
        }),
        "rename" => from_io(std::fs::rename(a(1), a(2)), |()| Value::Null),
        "hardlink" => from_io(std::fs::hard_link(a(1), a(2)), |()| Value::Null),
        "symlink" => from_io(std::os::unix::fs::symlink(a(1), a(2)), |()| Value::Null),
        "mkfifo" => {
            // SAFETY: a NUL-terminated path.
            let r = unsafe { libc::mkfifo(c(a(1)).as_ptr(), 0o600) };
            result(r == 0, if r == 0 { 0 } else { errno() }, Value::Null)
        }
        "mknod-dev" => {
            // SAFETY: a NUL-terminated path; a character device number that is harmless to name.
            let r = unsafe {
                libc::mknod(c(a(1)).as_ptr(), libc::S_IFCHR | 0o600, libc::makedev(1, 3))
            };
            result(r == 0, if r == 0 { 0 } else { errno() }, Value::Null)
        }
        "bind-unix" => from_io(std::os::unix::net::UnixListener::bind(a(1)), |_| {
            Value::Null
        }),
        "exec" => {
            let e = std::os::unix::process::CommandExt::exec(&mut std::process::Command::new(a(1)));
            result(false, e.raw_os_error().unwrap_or(-1), Value::Null)
        }
        "connect-unix" => match UnixStream::connect(a(1)) {
            Ok(mut s) => {
                let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = s.write_all(a(2).as_bytes());
                let mut reply = Vec::new();
                let mut buf = [0u8; 512];
                if let Ok(n) = s.read(&mut buf) {
                    reply.extend_from_slice(&buf[..n]);
                }
                result(true, 0, json!(String::from_utf8_lossy(&reply)))
            }
            Err(e) => result(false, e.raw_os_error().unwrap_or(-1), Value::Null),
        },
        "connect-abstract" => {
            // SAFETY: a plain socket call; the address is built below within its bounds.
            let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
            if fd < 0 {
                result(false, errno(), Value::Null)
            } else {
                let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
                addr.sun_family = libc::AF_UNIX as u16;
                let name = a(1).as_bytes();
                for (i, b) in name.iter().enumerate().take(100) {
                    addr.sun_path[i + 1] = *b as libc::c_char;
                }
                let len = (2 + 1 + name.len().min(100)) as u32;
                // SAFETY: `addr` is a fully initialised sockaddr_un and `len` does not exceed it.
                let r =
                    unsafe { libc::connect(fd, &addr as *const _ as *const libc::sockaddr, len) };
                let e = errno();
                // SAFETY: the descriptor created above.
                unsafe { libc::close(fd) };
                result(r == 0, if r == 0 { 0 } else { e }, Value::Null)
            }
        }
        "tcp" => {
            let addr: Result<std::net::SocketAddr, _> = a(1).parse();
            match addr {
                Ok(addr) => from_io(
                    std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(1)),
                    |_| Value::Null,
                ),
                Err(_) => result(false, -2, json!("bad address")),
            }
        }
        "socket" => {
            let domain: i32 = a(1).parse().unwrap_or(-1);
            // SAFETY: a plain socket call.
            let fd = unsafe { libc::socket(domain, libc::SOCK_STREAM, 0) };
            let e = errno();
            if fd >= 0 {
                // SAFETY: the descriptor created above.
                unsafe { libc::close(fd) };
            }
            result(fd >= 0, if fd >= 0 { 0 } else { e }, Value::Null)
        }
        // Descriptors above stderr that this process holds, with what each one is.
        "fds" => from_io(std::fs::read_dir("/proc/self/fd"), |d| {
            let mut fds: Vec<Value> = Vec::new();
            for e in d.flatten() {
                let n: i32 = e.file_name().to_string_lossy().parse().unwrap_or(-1);
                if n > 2 {
                    let target = std::fs::read_link(e.path())
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    fds.push(json!({"fd": n, "target": target}));
                }
            }
            json!(fds)
        }),
        "read-fd" => {
            let fd: i32 = a(1).parse().unwrap_or(-1);
            let mut buf = [0u8; 256];
            // SAFETY: reading into a local buffer within its length.
            let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
            let e = errno();
            result(
                n >= 0,
                if n >= 0 { 0 } else { e },
                json!(String::from_utf8_lossy(&buf[..n.max(0) as usize])),
            )
        }
        "env" => {
            json!({"ok": true, "errno": 0, "detail": std::env::vars().collect::<std::collections::BTreeMap<_, _>>()})
        }
        "kill" => {
            let pid: i32 = a(1).parse().unwrap_or(0);
            let sig: i32 = a(2).parse().unwrap_or(0);
            // SAFETY: a plain kill call; signal 0 only tests existence.
            let r = unsafe { libc::kill(pid, sig) };
            result(r == 0, if r == 0 { 0 } else { errno() }, Value::Null)
        }
        "ptrace" => {
            let pid: i32 = a(1).parse().unwrap_or(1);
            // SAFETY: a plain ptrace call that only attaches.
            let r = unsafe { libc::ptrace(libc::PTRACE_ATTACH, pid, 0, 0) };
            result(r == 0, if r == 0 { 0 } else { errno() }, Value::Null)
        }
        // One forbidden kernel entry each; the seccomp filter ends the process or refuses.
        "syscall" => {
            let r = match a(1) {
                // SAFETY (all arms): raw calls with harmless or zero arguments; the point is the
                // answer of the filter.
                "unshare-user" => unsafe { libc::unshare(libc::CLONE_NEWUSER) as i64 },
                "unshare-net" => unsafe { libc::unshare(libc::CLONE_NEWNET) as i64 },
                "mount" => unsafe {
                    libc::mount(
                        c("none").as_ptr(),
                        c("/tmp").as_ptr(),
                        c("tmpfs").as_ptr(),
                        0,
                        std::ptr::null(),
                    ) as i64
                },
                "io_uring" => unsafe {
                    libc::syscall(libc::SYS_io_uring_setup, 8, std::ptr::null::<u8>())
                },
                "bpf" => unsafe { libc::syscall(libc::SYS_bpf, 0, std::ptr::null::<u8>(), 0) },
                "clone3" => unsafe { libc::syscall(libc::SYS_clone3, std::ptr::null::<u8>(), 0) },
                "clone-newuser" => unsafe {
                    libc::syscall(
                        libc::SYS_clone,
                        libc::CLONE_NEWUSER as u64 | libc::SIGCHLD as u64,
                        0,
                        0,
                        0,
                        0,
                    )
                },
                "keyctl" => unsafe { libc::syscall(libc::SYS_keyctl, 0, 0, 0, 0, 0) },
                "process_vm_readv" => unsafe {
                    libc::syscall(
                        libc::SYS_process_vm_readv,
                        1,
                        std::ptr::null::<u8>(),
                        0,
                        std::ptr::null::<u8>(),
                        0,
                        0,
                    )
                },
                "setns" => unsafe { libc::syscall(libc::SYS_setns, 0, 0) },
                _ => -2,
            };
            result(r >= 0, if r >= 0 { 0 } else { errno() }, json!(r))
        }
        "status" => {
            let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
            let pick = |key: &str| {
                text.lines()
                    .find_map(|l| l.strip_prefix(key))
                    .map(|v| v.trim().to_owned())
                    .unwrap_or_default()
            };
            json!({"ok": true, "errno": 0, "detail": {
                "CapEff": pick("CapEff:"), "CapBnd": pick("CapBnd:"),
                "NoNewPrivs": pick("NoNewPrivs:"), "Seccomp": pick("Seccomp:"),
                "Uid": pick("Uid:"), "NSpid": pick("NSpid:"),
            }})
        }
        // A provider refreshing its token the way real ones do: write beside, then rename over.
        "atomic-auth" => {
            let dir = Path::new(a(1));
            let tmp = dir.join(".auth.json.tmp");
            let dest = dir.join("auth.json");
            let r = std::fs::write(&tmp, a(2))
                .and_then(|()| std::fs::rename(&tmp, &dest))
                .and_then(|()| std::fs::read_to_string(&dest));
            from_io(r, |s| json!(s))
        }
        "sleep" => {
            std::thread::sleep(Duration::from_millis(a(1).parse().unwrap_or(0)));
            result(true, 0, Value::Null)
        }
        // The grandchild that outlives its parent and keeps writing: the quiescence test.
        "double-fork" => {
            let target = a(1).to_owned();
            let delay: u64 = a(2).parse().unwrap_or(1000);
            // SAFETY: fork in a single-threaded process; the child only sleeps and writes.
            match unsafe { libc::fork() } {
                0 => {
                    // SAFETY: detach from the parent's session.
                    unsafe { libc::setsid() };
                    std::thread::sleep(Duration::from_millis(delay));
                    let _ = std::fs::write(&target, "the grandchild was still running");
                    // SAFETY: leave without running destructors of the parent's state.
                    unsafe { libc::_exit(0) };
                }
                -1 => result(false, errno(), Value::Null),
                _ => result(true, 0, Value::Null),
            }
        }
        "who" => result(
            true,
            0,
            json!({"pid": std::process::id(), "ppid": std::os::unix::process::parent_id(),
                   "cwd": std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
                   "exe_name": std::env::current_exe().map(|p| p.as_os_str().as_bytes().len()).unwrap_or(0)}),
        ),
        other => result(false, -1, json!(format!("unknown action {other}"))),
    };
    println!("{out}");
}
