use super::*;
use std::{io::Read, process::Stdio};

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
fn a_reaped_or_mismatched_handle_never_signals_anything() {
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
    drop(child);
    // Gone: either the PID has no process any more or it has a different start time.
    assert!(proc_start_ticks(pid).is_ok_and(|t| t != start) || proc_start_ticks(pid).is_err());
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
