//! M1-12: the checked, non-starting owner against a real disposable host process-in-a-thread on
//! a socket in a directory made for the test. The real host of the user is never named: every
//! path here is explicit, and the owner has no way to start one.
use super::*;
use crate::{
    host::{core::HostCore, protocol, server},
    projects::owner::{HostSocket, LaunchError, NoManaged, RealOwner, parse_start_ticks},
};
use std::{os::unix::net::UnixListener, thread, time::Duration};

struct Host {
    socket: PathBuf,
    thread: Option<thread::JoinHandle<()>>,
    _dir: TestDir,
}

impl Host {
    fn start() -> Self {
        let dir = TestDir::new();
        Self::start_in(dir)
    }

    fn start_in(dir: TestDir) -> Self {
        let socket = dir.0.join("host.sock");
        let core = HostCore::new(None);
        core.use_shell(PathBuf::from("/bin/sh"));
        let listener = server::bind(&socket).ok().expect("bind");
        let thread = Some(thread::spawn(move || {
            server::run(listener, core, Duration::from_secs(600));
        }));
        Self {
            socket,
            thread,
            _dir: dir,
        }
    }

    fn stop(&mut self) {
        let _ = HostSocket::new(self.socket.clone())
            .host_view(None)
            .map(|_| ());
        // `shutdown` is the host's own stop word.
        if let Ok(mut stream) = std::os::unix::net::UnixStream::connect(&self.socket) {
            let _ = protocol::write_frame(
                &mut stream,
                &protocol::Request {
                    op: "shutdown".into(),
                    params: Value::Null,
                },
            );
            let _ = protocol::read_frame::<protocol::Answer>(&mut stream);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop();
    }
}

#[test]
fn no_host_is_offline_and_none_is_started_or_substituted() {
    let dir = TestDir::new();
    let socket = dir.0.join("host.sock");
    let owner = RealOwner::new(Some(HostSocket::new(socket.clone())), NoManaged);
    assert_eq!(owner.host(None).unwrap_err(), OwnerError::HostOffline);
    assert_eq!(
        owner.launch_shell("/tmp", "h1:1:1:1:1").unwrap_err(),
        LaunchError::NotDispatched(OwnerError::HostOffline)
    );
    assert!(!socket.exists(), "reading the host never creates one");
    assert!(std::fs::read_dir(&dir.0).unwrap().next().is_none());
    // No socket path at all is the same answer.
    let none = RealOwner::new(None, NoManaged);
    assert_eq!(none.host(None).unwrap_err(), OwnerError::HostOffline);
}

#[test]
fn a_running_host_is_read_with_a_stable_epoch_and_its_features() {
    let host = Host::start();
    let owner = RealOwner::new(Some(HostSocket::new(host.socket.clone())), NoManaged);
    let first = owner.host(None).unwrap();
    let second = owner.host(Some(&first.epoch)).unwrap();
    assert_eq!(first.epoch, second.epoch);
    assert!(first.epoch.starts_with("h1:") && first.epoch.len() <= 256);
    assert!(first.features.iter().any(|f| f == "profile_effort"));
    assert!(first.terminals.is_empty());
}

#[test]
fn a_replaced_host_is_a_different_host_and_the_old_expectation_fails_without_a_call() {
    let mut first = Host::start();
    let owner = RealOwner::new(Some(HostSocket::new(first.socket.clone())), NoManaged);
    let old = owner.host(None).unwrap().epoch;
    let dir = TestDir(first._dir.0.clone());
    first.stop();
    // This fixture uses threads in one process. Keep the retired socket inode allocated
    // so the next thread cannot accidentally reuse the same (dev, inode, pid, start_ticks)
    // epoch tuple; a real replacement host has a distinct process identity.
    std::fs::rename(&first.socket, first._dir.0.join("retired-host.sock")).unwrap();
    assert_eq!(owner.host(Some(&old)).unwrap_err(), OwnerError::HostOffline);
    // Another host takes the same path.
    let _second = Host::start_in(dir);
    let now = owner.host(None).unwrap().epoch;
    assert_ne!(
        now, old,
        "a new process behind the same path is a new epoch"
    );
    assert_eq!(owner.host(Some(&old)).unwrap_err(), OwnerError::HostChanged);
    // A launch bound to the old epoch is refused before it is sent.
    assert_eq!(
        owner.launch_shell("/tmp", &old).unwrap_err(),
        LaunchError::NotDispatched(OwnerError::HostChanged)
    );
    assert!(
        owner.host(Some(&now)).unwrap().terminals.is_empty(),
        "nothing was created"
    );
}

#[test]
fn a_shell_launched_through_the_checked_owner_shows_in_the_next_read() {
    let host = Host::start();
    let owner = RealOwner::new(Some(HostSocket::new(host.socket.clone())), NoManaged);
    let epoch = owner.host(None).unwrap().epoch;
    let launched = owner.launch_shell("/tmp", &epoch).unwrap();
    let view = owner.host(Some(&epoch)).unwrap();
    let row = view
        .terminals
        .iter()
        .find(|t| t.terminal_id == launched.terminal_id)
        .expect("listed");
    assert!(row.running && row.native_session.is_none());
    // And the owner closes nothing and starts no second one by being read again.
    assert_eq!(owner.host(Some(&epoch)).unwrap().terminals.len(), 1);
}

/// A host from before features existed: it answers `hello` without `features`. It is still
/// browsable, and a missing feature disables only what needs it.
#[test]
fn an_old_host_without_features_is_still_browsable() {
    let dir = TestDir::new();
    let socket = dir.0.join("host.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    std::os::unix::fs::PermissionsExt::set_mode(
        &mut std::fs::metadata(&socket).unwrap().permissions(),
        0o600,
    );
    let server = thread::spawn(move || {
        // Three connections carry a request (hello, then the terminal list); the one in between
        // is the owner checking who answers, and sends nothing.
        let mut served = 0;
        while served < 2 {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let Ok(request) = protocol::read_frame::<protocol::Request>(&mut stream) else {
                continue;
            };
            served += 1;
            let answer = match request.op.as_str() {
                "hello" => json!({"protocol": 1, "version": "0.1.0", "pid": 1, "native": false}),
                "term_list" => {
                    json!([{"terminal": {"terminal_id": "aaaaaaaa-0000-4000-8000-000000000001",
                                                       "state": "running"},
                                       "title": "t", "created_at": 1, "native": null}])
                }
                _ => Value::Null,
            };
            let _ = protocol::write_frame(&mut stream, &protocol::Answer::Ok(answer));
        }
    });
    let owner = RealOwner::new(Some(HostSocket::new(socket)), NoManaged);
    let view = owner.host(None).unwrap();
    assert!(view.features.is_empty());
    assert_eq!(view.terminals.len(), 1);
    server.join().unwrap();
}

#[test]
fn a_host_that_speaks_another_protocol_is_unsupported_not_guessed_at() {
    let dir = TestDir::new();
    let socket = dir.0.join("host.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let _ = protocol::read_frame::<protocol::Request>(&mut stream);
            let _ =
                protocol::write_frame(&mut stream, &protocol::Answer::Ok(json!({"protocol": 2})));
        }
    });
    let owner = RealOwner::new(Some(HostSocket::new(socket)), NoManaged);
    assert_eq!(owner.host(None).unwrap_err(), OwnerError::Unsupported);
    server.join().unwrap();
}

#[test]
fn the_process_start_time_is_read_from_the_stat_line_whatever_the_command_is_called() {
    let tail =
        "S 1 1 1 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 987654 12345678 100 18446744073709551615";
    for command in ["bash", "a b", "x) S 5 (y", ")))"] {
        assert_eq!(
            parse_start_ticks(&format!("42 ({command}) {tail}")),
            Some(987654),
            "{command}"
        );
    }
    assert_eq!(parse_start_ticks("garbage"), None);
    assert_eq!(parse_start_ticks("1 (x) S 1"), None);
    // The running test process has one.
    assert!(crate::projects::owner::start_ticks(std::process::id() as i32).is_some());
}

#[test]
fn roots_are_looked_at_without_following_a_final_link_and_the_home_folder_is_not_configurable() {
    use crate::projects::owner::{inspect_root, inspect_root_in, make_folder};
    let dir = TestDir::new();
    let real = dir.0.join("real");
    std::fs::create_dir(&real).unwrap();
    std::fs::create_dir(real.join(".git")).unwrap();
    let link = dir.0.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let canonical = std::fs::canonicalize(&real).unwrap();
    let info = inspect_root(canonical.to_str().unwrap()).unwrap();
    assert_eq!(info.kind, CheckoutKind::Git);
    assert!(info.native_eligible && info.config_eligible);
    assert_eq!(
        inspect_root(link.to_str().unwrap()).unwrap_err(),
        RootError::Ineligible
    );
    assert_eq!(
        inspect_root(dir.0.join("none").to_str().unwrap()).unwrap_err(),
        RootError::Missing
    );
    assert_eq!(inspect_root("relative").unwrap_err(), RootError::Ineligible);
    // Somebody else's folder is not a root, and the home folder takes Native but no configuration.
    assert_eq!(inspect_root("/").unwrap_err(), RootError::Ineligible);
    let home = inspect_root_in(canonical.to_str().unwrap(), Some(&canonical)).unwrap();
    assert!(home.native_eligible && !home.config_eligible);
    let inside = inspect_root_in(canonical.to_str().unwrap(), Some(&dir.0)).unwrap();
    assert!(inside.native_eligible && inside.config_eligible);
    // A file is not a root.
    let file = dir.0.join("file");
    std::fs::write(&file, b"x").unwrap();
    assert_eq!(
        inspect_root(file.to_str().unwrap()).unwrap_err(),
        RootError::Ineligible
    );
    // Folders are made exclusively, one plain basename at a time.
    let made = make_folder(canonical.to_str().unwrap(), "fresh").unwrap();
    assert!(std::path::Path::new(&made).is_dir());
    assert_eq!(
        make_folder(canonical.to_str().unwrap(), "fresh").unwrap_err(),
        FolderError::Exists
    );
    for bad in ["", ".", "..", "a/b", "a\0b", "a\nb"] {
        assert_eq!(
            make_folder(canonical.to_str().unwrap(), bad).unwrap_err(),
            FolderError::Name,
            "{bad:?}"
        );
    }
    assert!(
        std::fs::read_dir(&made).unwrap().next().is_none(),
        "no Git, no files"
    );
}

#[test]
fn a_host_replaced_between_the_greeting_and_the_terminal_list_is_never_described_with_the_new_ones_epoch()
 {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let dir = TestDir::new();
    let path = dir.0.join("swap.sock");
    let old = UnixListener::bind(&path).unwrap();
    let asked_new = Arc::new(AtomicUsize::new(0));
    let server = {
        let (path, asked_new) = (path.clone(), asked_new.clone());
        thread::spawn(move || {
            // Connection 1 only observes the host: nothing is sent.
            let _ = old.accept();
            // Connection 2 is the greeting. Before it is answered the socket is replaced by
            // another one, as a restarted host would be.
            let (mut greeted, _) = old.accept().unwrap();
            let _: protocol::Request = protocol::read_frame(&mut greeted).unwrap();
            std::fs::remove_file(&path).unwrap();
            let new = UnixListener::bind(&path).unwrap();
            protocol::write_frame(
                &mut greeted,
                &protocol::Answer::Ok(json!({"protocol": protocol::PROTOCOL, "features": []})),
            )
            .unwrap();
            // Whatever the new host is asked is counted; it would answer an empty list.
            new.set_nonblocking(true).unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while std::time::Instant::now() < deadline {
                if let Ok((mut stream, _)) = new.accept() {
                    stream.set_nonblocking(false).unwrap();
                    if protocol::read_frame::<protocol::Request>(&mut stream).is_ok() {
                        asked_new.fetch_add(1, Ordering::SeqCst);
                        let _ =
                            protocol::write_frame(&mut stream, &protocol::Answer::Ok(json!([])));
                    }
                }
                thread::sleep(Duration::from_millis(5));
            }
        })
    };
    let socket = HostSocket::new(path);
    let first = socket.epoch().unwrap();
    let result = socket.host_view(Some(&first));
    assert_eq!(result.unwrap_err(), OwnerError::HostChanged);
    server.join().unwrap();
    assert_eq!(
        asked_new.load(Ordering::SeqCst),
        0,
        "nothing was asked of the host that replaced it"
    );
}

#[test]
fn a_write_the_host_refuses_as_busy_is_offered_again_and_other_refusals_are_not() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    for (refusals, word, expected_calls, outcome) in [
        (3usize, "busy", 4usize, true),
        // Never takes it: the attempt ends, after the bounded number of offers.
        (1000, "busy", 41, false),
        // Any other refusal is final at once.
        (1000, "unknown_terminal", 1, false),
    ] {
        let dir = TestDir::new();
        let path = dir.0.join("busy.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let server = {
            let calls = calls.clone();
            thread::spawn(move || {
                // Connection 1 only observes the host; each later one is a write.
                let _ = listener.accept();
                listener.set_nonblocking(true).unwrap();
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                while std::time::Instant::now() < deadline {
                    let Ok((mut stream, _)) = listener.accept() else {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    };
                    stream.set_nonblocking(false).unwrap();
                    if protocol::read_frame::<protocol::Request>(&mut stream).is_err() {
                        continue;
                    }
                    let n = calls.fetch_add(1, Ordering::SeqCst) + 1;
                    let answer = if n <= refusals {
                        protocol::Answer::Err(protocol::HostError::new(word))
                    } else {
                        protocol::Answer::Ok(json!({"written": 3}))
                    };
                    let _ = protocol::write_frame(&mut stream, &answer);
                    if n >= expected_calls {
                        break;
                    }
                }
            })
        };
        let owner = RealOwner::new(Some(HostSocket::new(path.clone())), NoManaged);
        let epoch = HostSocket::new(path).epoch().unwrap();
        let done = owner.term_write(&epoch, "aaaaaaaa-0000-4000-8000-000000000001", b"abc");
        assert_eq!(done.is_ok(), outcome, "{word} x{refusals}");
        if !outcome {
            assert_eq!(done.unwrap_err(), OwnerError::Refused);
        }
        server.join().unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            expected_calls,
            "{word} x{refusals}"
        );
    }
}
