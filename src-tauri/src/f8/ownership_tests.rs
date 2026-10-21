use super::*;
use std::{io::Read, os::fd::AsFd, process::Stdio};

fn quick() -> Grace {
    Grace {
        eof: Duration::from_millis(150),
        term: Duration::from_millis(150),
        kill: Duration::from_millis(1500),
    }
}
fn sh(script: &str) -> Command {
    let mut c = Command::new("/bin/sh");
    c.arg("-c")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    c
}
fn drive(child: &mut OwnedChild) -> (Progress, Vec<Stage>) {
    let mut stages = Vec::new();
    child.begin_stop(Instant::now());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let p = child.poll_stop(Instant::now()).unwrap();
        if let Progress::Pending(s) = p
            && stages.last() != Some(&s)
        {
            stages.push(s);
        }
        if !matches!(p, Progress::Pending(_)) || Instant::now() > deadline {
            return (p, stages);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
/// A bystander the test owns itself: same executable path as the owned child, never signalled.
fn bystander() -> Child {
    Command::new("/bin/sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .unwrap()
}
fn alive(c: &mut Child) -> bool {
    c.try_wait().unwrap().is_none()
}
fn owned_sleep() -> OwnedChild {
    let mut c = Command::new("/bin/sleep");
    c.arg("30").stdin(Stdio::piped()).stdout(Stdio::null());
    OwnedChild::spawn(&mut c, ContainmentPolicy::DirectChildOnly, quick())
        .unwrap()
        .0
}

#[test]
fn eof_ends_a_cooperative_child_without_any_signal() {
    let (mut child, report) = OwnedChild::spawn(
        &mut sh("cat >/dev/null"),
        ContainmentPolicy::DirectChildOnly,
        Grace {
            eof: Duration::from_secs(5),
            ..quick()
        },
    )
    .unwrap();
    assert_eq!(
        report,
        ContainmentReport {
            direct_child_pdeathsig: true,
            own_session: true,
            descendants_contained: false
        }
    );
    child.verify().unwrap();
    let (end, stages) = drive(&mut child);
    assert_eq!(end, Progress::Reaped(0));
    assert!(!stages.contains(&Stage::TermSent) && !stages.contains(&Stage::KillSent));
}

#[test]
fn a_child_ignoring_eof_and_term_is_killed_and_reaped_through_the_owned_handle_only() {
    let mut other = bystander();
    let (mut child, _) = OwnedChild::spawn(
        &mut sh("trap '' TERM; while true; do sleep 0.05; done"),
        ContainmentPolicy::DirectChildOnly,
        quick(),
    )
    .unwrap();
    let (end, stages) = drive(&mut child);
    assert_eq!(end, Progress::Reaped(128 + 9));
    assert_eq!(stages, [Stage::EofSent, Stage::TermSent, Stage::KillSent]);
    // The unrelated process with the same executable path was never touched.
    assert!(alive(&mut other));
    other.kill().unwrap();
    other.wait().unwrap();
}

#[test]
fn a_reaped_handle_never_signals_and_verify_detects_a_changed_start_time() {
    let mut other = bystander();
    let mut child = owned_sleep();
    child.verify().unwrap();
    // A recycled PID would show a different start time: the handle then refuses.
    child.start_ticks += 1;
    assert_eq!(child.verify(), Err(E::StaleRevision));
    child.start_ticks -= 1;
    child.verify().unwrap();
    let (end, _) = drive(&mut child);
    assert_eq!(end, Progress::Reaped(128 + 15));
    assert!(child.is_reaped());
    assert_eq!(child.verify(), Err(E::StaleRevision));
    assert_eq!(child.signal(libc::SIGKILL), Err(E::StaleRevision));
    // Repeating the stop is harmless and reports the same reap.
    assert_eq!(
        child.poll_stop(Instant::now()).unwrap(),
        Progress::Reaped(128 + 15)
    );
    assert!(alive(&mut other));
    other.kill().unwrap();
    other.wait().unwrap();
}

#[test]
fn dropping_an_owned_child_reaps_it_and_spares_everything_else() {
    let mut other = bystander();
    let child = owned_sleep();
    let pid = child.pid_for_diagnostics();
    let start = child.start_ticks;
    let began = Instant::now();
    drop(child);
    assert!(
        began.elapsed() < Duration::from_millis(500),
        "Drop must never synchronously wait for process exit"
    );
    // The background reaper owns the child until actual wait/reap, without stalling
    // the original caller. Observe with a bounded timeout, not a synchronous wait.
    let deadline = Instant::now() + Duration::from_secs(5);
    while proc_start_ticks(pid).is_ok_and(|ticks| ticks == start) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        proc_start_ticks(pid).is_ok_and(|ticks| ticks != start) || proc_start_ticks(pid).is_err(),
        "the background reaper must eventually collect a killable owned child"
    );
    assert!(alive(&mut other));
    other.kill().unwrap();
    other.wait().unwrap();
}

#[test]
fn descendants_are_reported_but_never_signalled_and_full_containment_is_refused() {
    let (mut child, report) = OwnedChild::spawn(
        &mut sh("sleep 30 & echo $!; wait"),
        ContainmentPolicy::DirectChildOnly,
        quick(),
    )
    .unwrap();
    assert!(!report.descendants_contained);
    let mut out = child.take_stdout().unwrap();
    let mut line = [0u8; 16];
    let n = out.read(&mut line).unwrap();
    let grandchild: i32 = std::str::from_utf8(&line[..n])
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(
        (0..100).any(|_| {
            std::thread::sleep(Duration::from_millis(10));
            child.descendant_count() >= 1
        }),
        "the walk reports the descendant"
    );
    // The test (not the code under test) pins the grandchild by pidfd to clean it up exactly.
    // SAFETY: integer-only syscall; the fd is wrapped immediately.
    let pinned = unsafe { libc::syscall(libc::SYS_pidfd_open, grandchild, 0) };
    assert!(pinned >= 0);
    // SAFETY: fresh descriptor from the kernel, owned by nobody else.
    let pinned = unsafe { OwnedFd::from_raw_fd(pinned as i32) };
    let (end, _) = drive(&mut child);
    assert!(matches!(end, Progress::Reaped(_)));
    // The direct child's death did not take the descendant with it: a reported gap.
    assert!(std::path::Path::new(&format!("/proc/{grandchild}")).exists());
    // SAFETY: signals only the test's own pinned grandchild through its pidfd.
    let r = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            pinned.as_raw_fd(),
            libc::SIGKILL,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        )
    };
    assert_eq!(r, 0);

    // Full descendant containment is not offered: refused before anything is spawned.
    let marker = std::env::temp_dir().join(format!("wyss-f8-own-{}", std::process::id()));
    let script = format!("touch {}", marker.display());
    let refused = OwnedChild::spawn(
        &mut sh(&script),
        ContainmentPolicy::DescendantsContained,
        quick(),
    );
    // A plain shell is not a verified namespace launcher.
    assert_eq!(refused.err(), Some(E::Unsupported));
    std::thread::sleep(Duration::from_millis(100));
    assert!(!marker.exists());
}

#[test]
fn a_failed_exec_is_an_error_not_a_dangling_owner() {
    let mut c = Command::new("/nonexistent/wyss-f8-binary");
    assert_eq!(
        OwnedChild::spawn(&mut c, ContainmentPolicy::DirectChildOnly, quick()).err(),
        Some(E::ProviderRejected)
    );
}

fn bwrap(extra: &[&str], payload: &str) -> Command {
    let mut c = Command::new(NAMESPACE_LAUNCHER);
    c.args(extra)
        .args(["--dev-bind", "/", "/", "--"])
        .arg("/bin/sh")
        .arg("-c")
        .arg(payload)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    c
}
fn wait_for(mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while !f() {
        assert!(Instant::now() < end, "timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn full_containment_is_granted_only_to_the_verified_namespace_launcher() {
    let marker = std::env::temp_dir().join(format!("wyss-f8-ns-{}", std::process::id()));
    let touch = format!("touch {}", marker.display());
    // Not the launcher, missing flags, flags only inside the payload, or sharing the PID space.
    let mut spoof = Command::new("/bin/sh");
    spoof.args(["-c", &touch, "--unshare-pid", "--die-with-parent", "--"]);
    let mut late = Command::new(NAMESPACE_LAUNCHER);
    late.args([
        "--dev-bind",
        "/",
        "/",
        "--",
        "/bin/sh",
        "-c",
        &touch,
        "--unshare-pid",
        "--die-with-parent",
    ]);
    let mut shared = bwrap(
        &["--unshare-pid", "--die-with-parent", "--share-pid"],
        &touch,
    );
    let mut half = bwrap(&["--die-with-parent"], &touch);
    let mut no_separator = Command::new(NAMESPACE_LAUNCHER);
    no_separator.args(["--unshare-pid", "--die-with-parent", "/bin/true"]);
    for c in [
        &mut spoof,
        &mut late,
        &mut shared,
        &mut half,
        &mut no_separator,
    ] {
        assert_eq!(
            OwnedChild::spawn(c, ContainmentPolicy::DescendantsContained, quick()).err(),
            Some(E::Unsupported)
        );
    }
    std::thread::sleep(Duration::from_millis(100));
    assert!(!marker.exists(), "a refused launch runs nothing");
}

#[test]
fn a_pid_namespace_takes_every_descendant_down_with_the_owned_child() {
    if !std::path::Path::new(NAMESPACE_LAUNCHER).exists() {
        return;
    }
    let mut c = bwrap(
        &["--unshare-pid", "--die-with-parent"],
        "sleep 30 & sleep 31 & echo ready; wait",
    );
    let (mut child, _) =
        OwnedChild::spawn(&mut c, ContainmentPolicy::DescendantsContained, quick()).unwrap();
    let mut out = child.take_stdout().unwrap();
    let flags = nix::fcntl::fcntl(out.as_raw_fd(), nix::fcntl::FcntlArg::F_GETFL).unwrap();
    nix::fcntl::fcntl(
        out.as_raw_fd(),
        nix::fcntl::FcntlArg::F_SETFL(
            nix::fcntl::OFlag::from_bits_truncate(flags) | nix::fcntl::OFlag::O_NONBLOCK,
        ),
    )
    .unwrap();
    let mut buf = [0u8; 16];
    let mut ready = Vec::new();
    wait_for(|| {
        match out.read(&mut buf) {
            Ok(0) => panic!("namespace payload exited before reporting ready"),
            Ok(n) => ready.extend_from_slice(&buf[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) => {}
            Err(e) => panic!("namespace payload readiness read failed: {e}"),
        }
        ready.ends_with(b"\n")
    });
    assert_eq!(ready, b"ready\n");
    // The payload lives in another PID namespace: observed, not assumed.
    wait_for(|| child.namespace_contained() && child.descendant_count() >= 3);
    // Pin everything under the owned launcher by pidfd (the test's own artifacts) ...
    let mut pinned = Vec::new();
    fn walk(pid: i32, depth: usize, out: &mut Vec<OwnedFd>) {
        let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
            return;
        };
        for t in tasks.flatten() {
            let kids = std::fs::read_to_string(t.path().join("children")).unwrap_or_default();
            for k in kids
                .split_whitespace()
                .filter_map(|k| k.parse::<i32>().ok())
            {
                // SAFETY: integer-only syscall; ownership of the fd moves into OwnedFd.
                let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, k, 0) };
                if fd >= 0 {
                    out.push(unsafe { OwnedFd::from_raw_fd(fd as i32) });
                }
                if depth > 0 {
                    walk(k, depth - 1, out);
                }
            }
        }
    }
    walk(child.pid_for_diagnostics(), 4, &mut pinned);
    assert!(pinned.len() >= 3);
    // ... stop only the owned launcher, and every one of them must end.
    let (end, _) = drive(&mut child);
    assert!(matches!(end, Progress::Reaped(_)));
    wait_for(|| {
        pinned.iter().all(|fd| {
            let mut p = [nix::poll::PollFd::new(
                fd.as_fd(),
                nix::poll::PollFlags::POLLIN,
            )];
            nix::poll::poll(&mut p, nix::poll::PollTimeout::ZERO).is_ok_and(|n| n == 1)
        })
    });
}

#[test]
fn reaper_capacity_refuses_before_spawn_and_releases_completed_slots() {
    // A local counter fixture exercises overflow without occupying global
    // reaper capacity or launching 4096 real processes in other test threads.
    let reaper = Arc::new(Reaper {
        slots: AtomicUsize::new(REAPER_LIMIT - 1),
        queue: Mutex::new(Vec::new()),
        wake: Condvar::new(),
    });
    let permit = reserve_slot(&reaper).unwrap();
    assert_eq!(reaper.slots.load(Ordering::Acquire), REAPER_LIMIT);
    assert!(matches!(reserve_slot(&reaper), Err(E::CapacityExceeded)));
    drop(permit);
    assert_eq!(reaper.slots.load(Ordering::Acquire), REAPER_LIMIT - 1);
    let again = reserve_slot(&reaper).unwrap();
    drop(again);
}

#[test]
fn failed_child_pin_never_executes_payload_and_releases_reap_permit() {
    let root = std::env::temp_dir().join(format!("wyss-f8-gate-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let marker = root.join("executed");
    let mut command = sh(&format!("touch '{}'; sleep 30", marker.display()));
    let reaper = Arc::new(Reaper {
        slots: AtomicUsize::new(0),
        queue: Mutex::new(Vec::new()),
        wake: Condvar::new(),
    });
    let permit = reserve_slot(&reaper).unwrap();
    assert_eq!(
        gated_spawn(&mut command, permit, quick(), true).err(),
        Some(E::ProviderRejected)
    );
    assert!(!marker.exists(), "an unpinned child must never reach exec");
    assert_eq!(reaper.slots.load(Ordering::Acquire), 0);
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn external_reap_releases_owned_slot_and_reports_unknown_status() {
    let reaper = Arc::new(Reaper {
        slots: AtomicUsize::new(0),
        queue: Mutex::new(Vec::new()),
        wake: Condvar::new(),
    });
    let mut command = sh("exec sleep 30");
    let mut child = gated_spawn(&mut command, reserve_slot(&reaper).unwrap(), quick(), false)
        .unwrap()
        .0;
    child.signal(libc::SIGKILL).unwrap();
    let mut status = 0;
    // Test alone consumes its own exact child's status, modeling a SIGCHLD collector.
    assert_eq!(
        unsafe { libc::waitpid(child.pid, &mut status, 0) },
        child.pid
    );
    assert_eq!(
        child.poll_stop(Instant::now()).unwrap(),
        Progress::Reaped(-1)
    );
    assert!(child.is_reaped());
    drop(child);
    assert_eq!(reaper.slots.load(Ordering::Acquire), 0);
}

#[test]
fn command_can_be_reused_after_a_gated_launch() {
    let mut command = sh("cat >/dev/null");
    for _ in 0..2 {
        let (mut child, _) =
            OwnedChild::spawn(&mut command, ContainmentPolicy::DirectChildOnly, quick()).unwrap();
        assert_eq!(drive(&mut child).0, Progress::Reaped(0));
    }
}

#[test]
fn caller_thread_exit_does_not_kill_coordinator_owned_child() {
    let mut child = std::thread::spawn(owned_sleep).join().unwrap();
    child.verify().unwrap();
    assert!(child.try_wait().unwrap().is_none());
    assert!(matches!(drive(&mut child).0, Progress::Reaped(_)));
}

#[test]
fn delayed_post_exec_handoff_kills_pinned_child_before_refusing_launch() {
    let root = std::env::temp_dir().join(format!("wyss-f8-gate-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let marker = root.join("late-effect");
    let mut command = sh(&format!(
        "sleep 0.3; touch '{}'; sleep 30",
        marker.display()
    ));
    let permit = reap_permit().unwrap();
    let began = Instant::now();
    assert_eq!(
        gated_spawn_timed(
            &mut command,
            permit,
            quick(),
            false,
            Duration::from_millis(50),
            Duration::from_millis(150)
        )
        .err(),
        Some(E::ProviderRejected)
    );
    assert!(began.elapsed() < Duration::from_millis(500));
    std::thread::sleep(Duration::from_millis(400));
    assert!(
        !marker.exists(),
        "handoff timeout must kill the exact pinned child before returning refusal"
    );
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn ownership_resource_diagnostics_for_reused_command_and_idle_threads() {
    fn fds() -> usize {
        std::fs::read_dir("/proc/self/fd").unwrap().count()
    }
    fn owned_threads() -> std::collections::BTreeMap<String, u64> {
        let mut found = std::collections::BTreeMap::new();
        for entry in std::fs::read_dir("/proc/self/task").unwrap().flatten() {
            let name = std::fs::read_to_string(entry.path().join("comm")).unwrap_or_default();
            if !name.starts_with("wyss-f8-owned") && !name.starts_with("wyss-f8-spawn") {
                continue;
            }
            let stat = std::fs::read_to_string(entry.path().join("stat")).unwrap();
            let fields: Vec<_> = stat
                .rsplit_once(')')
                .unwrap()
                .1
                .split_whitespace()
                .collect();
            let ticks = fields[11].parse::<u64>().unwrap() + fields[12].parse::<u64>().unwrap();
            found.insert(name.trim().to_owned(), ticks);
        }
        found
    }
    let mut command = sh("cat >/dev/null");
    // Initialize the two lifetime threads before taking descriptor/thread baselines.
    let (mut warmup, _) =
        OwnedChild::spawn(&mut command, ContainmentPolicy::DirectChildOnly, quick()).unwrap();
    assert_eq!(drive(&mut warmup).0, Progress::Reaped(0));
    drop(warmup);
    let before_fds = fds();
    let began = Instant::now();
    for _ in 0..50 {
        let (mut child, _) =
            OwnedChild::spawn(&mut command, ContainmentPolicy::DirectChildOnly, quick()).unwrap();
        assert_eq!(drive(&mut child).0, Progress::Reaped(0));
    }
    let launch_elapsed = began.elapsed();
    let after_fds = fds();
    let threads_before = owned_threads();
    std::thread::sleep(Duration::from_millis(250));
    let threads_after = owned_threads();
    let ticks: u64 = threads_after
        .iter()
        .map(|(name, after)| after.saturating_sub(*threads_before.get(name).unwrap_or(after)))
        .sum();
    // Metrics are deliberately reported rather than timing-gated: other tests
    // may concurrently own children. Root also executes this diagnostic alone.
    println!(
        "F8 ownership diagnostics: launches=50 elapsed_ms={} fd_before={} fd_after={} owned_threads={} idle_window_ms=250 cpu_ticks={}",
        launch_elapsed.as_millis(),
        before_fds,
        after_fds,
        threads_after.len(),
        ticks
    );
    assert_eq!(
        threads_after.len(),
        2,
        "one spawn coordinator and one reaper only"
    );
}

#[test]
fn pending_timeout_retains_owner_and_permit_until_delayed_handoff_is_reaped() {
    let reaper = Arc::new(Reaper {
        slots: AtomicUsize::new(0),
        queue: Mutex::new(Vec::new()),
        wake: Condvar::new(),
    });
    let mut pending = PendingOwnedSpawn::start(
        sh("exec sleep 30"),
        reserve_slot(&reaper).unwrap(),
        quick(),
        false,
        Duration::from_secs(5),
        Duration::from_millis(250),
    )
    .unwrap();
    wait_for(|| {
        assert!(pending.poll(Instant::now()).is_none());
        pending.refusal_pin.is_some()
    });
    let timed_out = pending.deadline + Duration::from_millis(1);
    assert!(pending.poll(timed_out).is_none());
    assert_eq!(pending.failure(), Some(E::ProviderRejected));
    assert!(!pending.finished());
    assert_eq!(reaper.slots.load(Ordering::Acquire), 1);
    wait_for(|| match pending.poll(timed_out) {
        Some(Err(E::ProviderRejected)) => true,
        None => false,
        _ => panic!("a timed-out spawn must not become a successful launch"),
    });
    assert!(pending.finished());
    assert_eq!(reaper.slots.load(Ordering::Acquire), 0);
}

#[test]
fn pending_failed_pin_never_executes_payload_and_completes_only_after_spawn_failure() {
    let root = std::env::temp_dir().join(format!("wyss-f8-pending-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let marker = root.join("executed");
    let mut pending = PendingOwnedSpawn::fixture(
        sh(&format!("touch '{}'; exec sleep 30", marker.display())),
        quick(),
        true,
        Duration::from_secs(5),
        Duration::ZERO,
    )
    .unwrap();
    wait_for(|| match pending.poll(Instant::now()) {
        Some(Err(E::ProviderRejected)) => true,
        None => false,
        _ => panic!("an unpinned payload must never become executable"),
    });
    assert!(pending.finished());
    assert!(!marker.exists());
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn child_transferred_pidfd_has_exact_identity_cloexec_and_survives_reap_without_retargeting() {
    let mut pending = PendingOwnedSpawn::fixture(
        sh("exec sleep 30"),
        quick(),
        false,
        Duration::from_secs(5),
        Duration::from_millis(100),
    )
    .unwrap();
    wait_for(|| {
        assert!(pending.poll(Instant::now()).is_none());
        pending.refusal_pin.is_some()
    });
    let fd = Arc::clone(pending.refusal_pin.as_ref().unwrap());
    let sender = i32::from_ne_bytes(pending.pid_bytes);
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", fd.as_raw_fd())).unwrap();
    assert_eq!(
        info.lines()
            .find_map(|line| line.strip_prefix("Pid:")?.trim().parse::<i32>().ok()),
        Some(sender)
    );
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) };
    assert!(flags >= 0 && flags & libc::FD_CLOEXEC != 0);
    let mut owned = None;
    wait_for(|| match pending.poll(Instant::now()) {
        Some(Ok((child, _))) => {
            owned = Some(child);
            true
        }
        None => false,
        Some(Err(error)) => panic!("self-pidfd transfer failed: {error:?}"),
    });
    let mut child = owned.unwrap();
    assert_eq!(child.pid_for_diagnostics(), sender);
    assert_eq!(fd.as_raw_fd(), child.pidfd.as_raw_fd());
    assert!(matches!(drive(&mut child).0, Progress::Reaped(_)));
    // The transferred handle is permanently bound to that dead child, even
    // after its numeric PID becomes available for reuse.
    let result = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            fd.as_raw_fd(),
            0,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        )
    };
    assert_eq!(result, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}

#[test]
fn pidfd_receiver_rejects_missing_duplicate_and_mismatched_rights_without_retention() {
    use std::os::unix::net::UnixStream;
    for (rights, reported) in [
        (0usize, std::process::id() as i32),
        (2, std::process::id() as i32),
        (1, i32::MAX),
    ] {
        let (parent, sender) = UnixStream::pair().unwrap();
        let (_reply, result) = mpsc::sync_channel(1);
        let mut pending = PendingOwnedSpawn {
            result,
            parent,
            pin: Arc::new(Mutex::new(None)),
            refusal_pin: None,
            received_pin: None,
            pid_bytes: [0; std::mem::size_of::<i32>()],
            received: 0,
            refused: false,
            deadline: Instant::now() + Duration::from_secs(5),
            failure: None,
            child: None,
            original: None,
            finished: true,
        };
        let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, libc::getpid(), 0) };
        assert!(raw >= 0);
        let fd = unsafe { OwnedFd::from_raw_fd(raw as i32) };
        let bytes = reported.to_ne_bytes();
        let mut iov = libc::iovec {
            iov_base: bytes.as_ptr().cast_mut().cast(),
            iov_len: bytes.len(),
        };
        let mut control = [0usize; 8];
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        if rights > 0 {
            msg.msg_control = control.as_mut_ptr().cast();
            msg.msg_controllen =
                unsafe { libc::CMSG_SPACE((rights * std::mem::size_of::<i32>()) as u32) } as usize;
            unsafe {
                let header = libc::CMSG_FIRSTHDR(&msg);
                (*header).cmsg_level = libc::SOL_SOCKET;
                (*header).cmsg_type = libc::SCM_RIGHTS;
                (*header).cmsg_len =
                    libc::CMSG_LEN((rights * std::mem::size_of::<i32>()) as u32) as usize;
                for index in 0..rights {
                    std::ptr::write_unaligned(
                        libc::CMSG_DATA(header).cast::<i32>().add(index),
                        fd.as_raw_fd(),
                    );
                }
            }
        }
        assert_eq!(
            unsafe { libc::sendmsg(sender.as_raw_fd(), &msg, libc::MSG_NOSIGNAL) },
            bytes.len() as isize
        );
        assert!(matches!(pending.receive_pin(), Err(E::ProviderRejected)));
        assert!(pending.received_pin.is_none());
    }
}

#[test]
fn authorized_isolation_descriptor_is_readable_after_exec() {
    let root = std::env::temp_dir().join(format!("wyss-f8-grant-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let source = root.join("grant");
    std::fs::write(&source, b"trusted-grant\n").unwrap();
    let grant = std::fs::File::open(&source).unwrap();
    let fd = grant.as_raw_fd();
    let command = sh(&format!("cat /proc/self/fd/{fd}"));
    let grant: OwnedFd = grant.into();
    let launch =
        crate::adapters::codex::probe::isolation::IsolationLaunch::fixture(command, vec![grant]);
    let mut pending =
        PendingOwnedSpawn::spawn_isolated(launch, ContainmentPolicy::DirectChildOnly, quick())
            .unwrap();
    let mut owned = None;
    wait_for(|| match pending.poll(Instant::now()) {
        Some(Ok((child, _))) => {
            owned = Some(child);
            true
        }
        Some(Err(e)) => panic!("grant launch refused: {e:?}"),
        None => false,
    });
    let mut child = owned.unwrap();
    let mut observed = String::new();
    child
        .take_stdout()
        .unwrap()
        .read_to_string(&mut observed)
        .unwrap();
    assert_eq!(drive(&mut child).0, Progress::Reaped(0));
    assert_eq!(observed, "trusted-grant\n");
    std::fs::remove_dir_all(root).unwrap();
}

// Duplicate before enqueueing: only this owned duplicate appears in launch argv.
fn grant_duplicate(file: &std::fs::File) -> OwnedFd {
    let raw = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 128) };
    assert!(raw >= 128);
    unsafe { OwnedFd::from_raw_fd(raw) }
}

#[test]
fn exec_grants_do_not_authorize_unapproved_fds_or_recycled_source_numbers() {
    let root = std::env::temp_dir().join(format!("wyss-f8-grants-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("grant"), b"original-inode\n").unwrap();
    std::fs::write(root.join("unrelated"), b"SUBSTITUTED\n").unwrap();
    let original = std::fs::File::open(root.join("grant")).unwrap();
    let replacement = std::fs::File::open(root.join("unrelated")).unwrap();
    let grant = grant_duplicate(&original);
    let unapproved = grant_duplicate(&replacement);
    // Simulate reuse of the original source's number while its independent,
    // trusted duplicate remains alive. dup3 atomically replaces our own FD;
    // there is no free-number race with other test threads.
    assert_eq!(
        unsafe {
            libc::dup3(
                replacement.as_raw_fd(),
                original.as_raw_fd(),
                libc::O_CLOEXEC,
            )
        },
        original.as_raw_fd()
    );
    assert_eq!(
        unsafe { libc::fcntl(unapproved.as_raw_fd(), libc::F_SETFD, 0) },
        0
    );
    let command = sh(&format!(
        "if cat /proc/self/fd/{} 2>/dev/null; then exit 90; fi; cat /proc/self/fd/{}",
        unapproved.as_raw_fd(),
        grant.as_raw_fd(),
    ));
    let launch =
        crate::adapters::codex::probe::isolation::IsolationLaunch::fixture(command, vec![grant]);
    drop(original);
    let mut pending =
        PendingOwnedSpawn::spawn_isolated(launch, ContainmentPolicy::DirectChildOnly, quick())
            .unwrap();
    let mut owned = None;
    wait_for(|| match pending.poll(Instant::now()) {
        Some(Ok((child, _))) => {
            owned = Some(child);
            true
        }
        Some(Err(e)) => panic!("unexpected refusal: {e:?}"),
        None => false,
    });
    let mut child = owned.unwrap();
    let mut observed = String::new();
    child
        .take_stdout()
        .unwrap()
        .read_to_string(&mut observed)
        .unwrap();
    assert_eq!(drive(&mut child).0, Progress::Reaped(0));
    assert_eq!(observed, "original-inode\n");
    println!(
        "post-exec: authorized=original-inode; unapproved=unreadable; substituted source=not inherited"
    );
    drop(pending);
    drop(unapproved);
    drop(replacement);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn closed_exec_grant_refuses_payload_and_releases_capacity_only_after_reap() {
    let root = std::env::temp_dir().join(format!("wyss-f8-closed-grant-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let marker = root.join("executed");
    let grant = grant_duplicate(&std::fs::File::open("/dev/null").unwrap());
    let fd = grant.as_raw_fd();
    let mut command = sh(&format!("touch '{}'", marker.display()));
    // Fault injection in the fork's private descriptor table. The parent's
    // OwnedFd remains valid; Rust ownership is never violated in this test.
    unsafe {
        command.pre_exec(move || {
            libc::close(fd);
            Ok(())
        });
    }
    let reaper = Arc::new(Reaper {
        slots: AtomicUsize::new(0),
        queue: Mutex::new(Vec::new()),
        wake: Condvar::new(),
    });
    let mut pending = PendingOwnedSpawn::start_with_grants(
        command,
        reserve_slot(&reaper).unwrap(),
        quick(),
        false,
        Duration::from_secs(5),
        Duration::from_millis(100),
        vec![grant],
    )
    .unwrap();
    assert_eq!(reaper.slots.load(Ordering::Acquire), 1);
    wait_for(|| match pending.poll(Instant::now()) {
        Some(Err(E::ProviderRejected)) => true,
        None => false,
        _ => panic!("closed grant must fail before exec"),
    });
    assert!(pending.finished());
    assert_eq!(reaper.slots.load(Ordering::Acquire), 0);
    assert!(!marker.exists());
    drop(pending);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cancelled_and_failed_grant_spawns_release_grants_and_reaper_permits() {
    use std::os::unix::net::UnixStream;
    for cancelled in [false, true] {
        let (peer, grant) = UnixStream::pair().unwrap();
        peer.set_nonblocking(true).unwrap();
        let grant: OwnedFd = grant.into();
        let reaper = Arc::new(Reaper {
            slots: AtomicUsize::new(0),
            queue: Mutex::new(Vec::new()),
            wake: Condvar::new(),
        });
        let mut command = Command::new(if cancelled {
            "/bin/sleep"
        } else {
            "/nonexistent/wyss-f8-exec"
        });
        command.arg("30");
        let mut pending = PendingOwnedSpawn::start_with_grants(
            command,
            reserve_slot(&reaper).unwrap(),
            quick(),
            false,
            Duration::from_secs(5),
            Duration::from_millis(150),
            vec![grant],
        )
        .unwrap();
        wait_for(|| {
            assert!(pending.poll(Instant::now()).is_none());
            pending.refusal_pin.is_some()
        });
        let pin = Arc::clone(pending.refusal_pin.as_ref().unwrap());
        if cancelled {
            pending.cancel();
        }
        assert!(!pending.finished());
        assert_eq!(reaper.slots.load(Ordering::Acquire), 1);
        wait_for(|| match pending.poll(Instant::now()) {
            Some(Err(E::ProviderRejected)) => true,
            None => false,
            _ => panic!("cancelled or failed exec must not succeed"),
        });
        assert_eq!(reaper.slots.load(Ordering::Acquire), 0);
        // Stable kernel identity proves the child has terminated, without a PID lookup.
        let mut pollfd = libc::pollfd {
            fd: pin.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        assert_eq!(unsafe { libc::poll(&mut pollfd, 1, 0) }, 1);
        assert_ne!(pollfd.revents & libc::POLLIN, 0);
        drop(pending);
        // Socket EOF is kernel evidence that no queued command, child or parent
        // owner still retains the grant (no fragile global descriptor counts).
        let mut peer = peer;
        wait_for(|| match peer.read(&mut [0u8; 1]) {
            Ok(0) => true,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => false,
            other => panic!("unexpected grant state: {other:?}"),
        });
    }
}

#[test]
fn invalid_unsealed_grant_is_rejected_before_enqueueing_payload() {
    let grant = grant_duplicate(&std::fs::File::open("/dev/null").unwrap());
    assert_eq!(
        unsafe { libc::fcntl(grant.as_raw_fd(), libc::F_SETFD, 0) },
        0
    );
    let launch = crate::adapters::codex::probe::isolation::IsolationLaunch::fixture(
        sh("exit 99"),
        vec![grant],
    );
    assert_eq!(
        PendingOwnedSpawn::spawn_isolated(launch, ContainmentPolicy::DirectChildOnly, quick(),)
            .err(),
        Some(E::InvalidRequest)
    );
}

#[test]
fn an_earlier_pre_exec_hook_cannot_authorize_a_plain_command_descriptor() {
    let grant = grant_duplicate(&std::fs::File::open("/dev/null").unwrap());
    let fd = grant.as_raw_fd();
    let mut command = sh(&format!(
        "if cat /proc/self/fd/{fd} 2>/dev/null; then echo leaked; else echo sealed; fi"
    ));
    unsafe {
        command.pre_exec(move || {
            if libc::fcntl(grant.as_raw_fd(), libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let (mut child, _) =
        OwnedChild::spawn(&mut command, ContainmentPolicy::DirectChildOnly, quick()).unwrap();
    let mut output = String::new();
    child
        .take_stdout()
        .unwrap()
        .read_to_string(&mut output)
        .unwrap();
    assert_eq!(drive(&mut child).0, Progress::Reaped(0));
    assert_eq!(output, "sealed\n");
}

#[test]
fn failure_of_the_blanket_cloexec_sweep_prevents_payload_exec() {
    let root = std::env::temp_dir().join(format!("wyss-f8-sweep-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let marker = root.join("executed");
    let mut command = sh(&format!("touch '{}'", marker.display()));
    let (mut probe, installed) = std::os::unix::net::UnixStream::pair().unwrap();
    // Install a tiny seccomp filter ONLY in the forked child. It makes the real
    // close_range syscall return ENOSYS; all other syscalls remain allowed.
    let mut filter = [
        libc::sock_filter {
            code: 0x20,
            jt: 0,
            jf: 0,
            k: 0,
        }, // LD syscall nr
        libc::sock_filter {
            code: 0x15,
            jt: 0,
            jf: 1,
            k: libc::SYS_close_range as u32,
        },
        libc::sock_filter {
            code: 0x06,
            jt: 0,
            jf: 0,
            k: libc::SECCOMP_RET_ERRNO | libc::ENOSYS as u32,
        },
        libc::sock_filter {
            code: 0x06,
            jt: 0,
            jf: 0,
            k: libc::SECCOMP_RET_ALLOW,
        },
    ];
    unsafe {
        command.pre_exec(move || {
            let program = libc::sock_fprog {
                len: filter.len() as u16,
                filter: filter.as_mut_ptr(),
            };
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
                || libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_FILTER, &program) != 0
            {
                return Err(std::io::Error::last_os_error());
            }
            if libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 4u32) != -1
                || *libc::__errno_location() != libc::ENOSYS
                || libc::write(installed.as_raw_fd(), b"1".as_ptr().cast(), 1) != 1
            {
                return Err(std::io::Error::from_raw_os_error(libc::EIO));
            }
            Ok(())
        });
    }
    assert_eq!(
        OwnedChild::spawn(&mut command, ContainmentPolicy::DirectChildOnly, quick()).err(),
        Some(E::ProviderRejected)
    );
    drop(command);
    let mut evidence = Vec::new();
    probe.read_to_end(&mut evidence).unwrap();
    assert_eq!(evidence, b"1", "seccomp must actually deny close_range");
    assert!(!marker.exists());
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn cancellation_before_gate_ack_never_executes_or_retains_a_grant() {
    use std::os::unix::net::UnixStream;
    let root = std::env::temp_dir().join(format!("wyss-f8-grant-cancel-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let marker = root.join("executed");
    let (mut peer, grant) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    let launch = crate::adapters::codex::probe::isolation::IsolationLaunch::fixture(
        sh(&format!("touch '{}'", marker.display())),
        vec![grant.into()],
    );
    let mut pending =
        PendingOwnedSpawn::spawn_isolated(launch, ContainmentPolicy::DirectChildOnly, quick())
            .unwrap();
    pending.cancel();
    wait_for(|| match pending.poll(Instant::now()) {
        Some(Err(E::ProviderRejected)) => true,
        None => false,
        _ => panic!("cancelled gate cannot authorize exec"),
    });
    assert!(!marker.exists());
    drop(pending);
    wait_for(|| match peer.read(&mut [0u8; 1]) {
        Ok(0) => true,
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => false,
        other => panic!("grant leaked: {other:?}"),
    });
    std::fs::remove_dir(root).unwrap();
}
