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
    let mut buf = [0u8; 16];
    wait_for(|| out.read(&mut buf).is_ok_and(|n| n > 0) || true);
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
