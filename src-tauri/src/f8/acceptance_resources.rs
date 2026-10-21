//! What the application costs: CPU while idle, memory, descriptors, threads and latency, and
//! the limits that keep them bounded. The table is printed; the ceilings asserted are loose
//! enough to hold when other tests run beside this one. Run alone for the table that counts:
//! `cargo test --lib f8::acceptance::resources -- --ignored --test-threads=1 --nocapture`.
use super::*;

fn ticks_per_second() -> f64 {
    // SAFETY: sysconf is a pure query.
    unsafe { nix::libc::sysconf(nix::libc::_SC_CLK_TCK) as f64 }
}
/// CPU time of one thread of this process, in seconds.
fn thread_cpu(tid: usize) -> f64 {
    let stat = std::fs::read_to_string(format!("/proc/self/task/{tid}/stat")).unwrap();
    // The command is in parentheses and may hold spaces: the fields after it are counted.
    let rest = &stat[stat.rfind(')').unwrap() + 2..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    (fields[11].parse::<f64>().unwrap() + fields[12].parse::<f64>().unwrap()) / ticks_per_second()
}
fn status_kib(path: &str, key: &str) -> u64 {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|v| v.split_whitespace().next()?.parse().ok())
        .unwrap_or(0)
}
fn fds() -> usize {
    std::fs::read_dir("/proc/self/fd").unwrap().count()
}
fn threads() -> u64 {
    status_kib("/proc/self/status", "Threads:")
}
fn rss_kib(pid: u32) -> u64 {
    status_kib(&format!("/proc/{pid}/status"), "VmRSS:")
}
fn percentile(samples: &mut [Duration], p: f64) -> Duration {
    samples.sort();
    samples[((samples.len() as f64 - 1.0) * p).round() as usize]
}
/// CPU of the application's own thread over `window`, as a fraction of one core.
fn idle_cpu(app: &App, window: Duration) -> f64 {
    let tid = eventually(|| Some(app.tid.load(Ordering::Acquire)).filter(|t| *t != 0));
    let (before, started) = (thread_cpu(tid), Instant::now());
    std::thread::sleep(window);
    (thread_cpu(tid) - before) / started.elapsed().as_secs_f64()
}

#[test]
fn an_idle_application_costs_almost_nothing_and_a_busy_one_answers_in_milliseconds() {
    let app = App::start(Options {
        limit: 1,
        tools: true,
        ..Options::default()
    });
    let alone = idle_cpu(&app, Duration::from_secs(2));
    let s = app.up(AgentMode::LocalTools);
    // A window that waits on the durable stream holds a watcher slot and asks for nothing.
    let stream = app.gui.state().authority.unwrap().stream_id;
    let head = app.gui.state().authority.unwrap().last_sequence;
    let waiter = {
        let gui = app.gui.clone();
        std::thread::spawn(move || gui.audit_wait(stream, head, 3_000))
    };
    let with_runtime = idle_cpu(&app, Duration::from_secs(2));
    let _ = waiter.join();
    eprintln!(
        "F8-PERF idle thread CPU: no runtime {:.2}% · one runtime and a watcher {:.2}%",
        alone * 100.0,
        with_runtime * 100.0
    );
    assert!(
        alone < 0.10,
        "an authority with nothing to do uses {alone:.3} of a core"
    );
    assert!(
        with_runtime < 0.10,
        "with one runtime it uses {with_runtime:.3} of a core"
    );

    // Latency of the calls the window makes, with a runtime running.
    let mut raw = app.raw();
    raw.hello();
    let mut status = Vec::new();
    for _ in 0..200 {
        let started = Instant::now();
        let answer = raw.send("authority/status", json!({}));
        assert!(answer["error"].is_null());
        status.push(started.elapsed());
    }
    let mut detail = Vec::new();
    for _ in 0..30 {
        let started = Instant::now();
        app.gui.session(s.id.clone()).unwrap();
        detail.push(started.elapsed());
    }
    let mut list = Vec::new();
    for _ in 0..30 {
        let started = Instant::now();
        app.gui.sessions(None).unwrap();
        list.push(started.elapsed());
    }
    let (p50, p99) = (percentile(&mut status, 0.5), percentile(&mut status, 0.99));
    let (d50, d99) = (percentile(&mut detail, 0.5), percentile(&mut detail, 0.99));
    let (l50, l99) = (percentile(&mut list, 0.5), percentile(&mut list, 0.99));
    eprintln!(
        "F8-PERF latency: one request p50 {p50:?} p99 {p99:?} · session detail p50 {d50:?} p99 {d99:?} · session list p50 {l50:?} p99 {l99:?}"
    );
    assert!(p99 < Duration::from_millis(100), "{p99:?}");
    assert!(d99 < Duration::from_millis(500), "{d99:?}");
    assert!(l99 < Duration::from_millis(500), "{l99:?}");

    // A turn, from the ask to its first output, and an approval decision, to its receipt.
    let intent = uuid();
    let started = Instant::now();
    let turn = settled(|| {
        app.gui.turn_start(
            intent.clone(),
            s.fence.clone(),
            s.detail.session.revision,
            "approve".into(),
        )
    })
    .id;
    let first = eventually(|| {
        let r = app.gui.turn(turn.clone(), None, true).ok()?;
        (!r.events.is_empty()).then_some(r)
    });
    let to_output = started.elapsed();
    let notice = eventually(|| app.gui.turn(turn.clone(), None, false).ok()?.approval);
    let approval = app.gui.approval(notice.id, None, None).unwrap();
    let started = Instant::now();
    settled(|| app.decide(&s, &approval, ApprovalDecision::Accept));
    let to_receipt = started.elapsed();
    eprintln!(
        "F8-PERF turn start to first output {to_output:?} · approval decision to receipt {to_receipt:?}"
    );
    assert!(!first.events.is_empty());
    assert!(to_output < Duration::from_secs(2));
    assert!(to_receipt < Duration::from_secs(2));
}

#[test]
fn connections_descriptors_and_threads_return_to_where_they_were() {
    let app = App::start(Options {
        limit: 1,
        ..Options::default()
    });
    let session = app.session();
    // Other tests run beside this one and open descriptors of their own: the figure that
    // counts is taken by the table test, alone. Here only a leak that grows with the number
    // of connections can show, and it would show as hundreds.
    let before = (fds(), threads());
    for _ in 0..200 {
        let mut raw = app.raw();
        raw.hello();
        raw.send("authority/status", json!({}));
    }
    for _ in 0..100 {
        let gui = F8Gui::local(HandleConnector(app.handle.clone()));
        gui.session(session.clone()).unwrap();
        gui.shutdown();
    }
    std::thread::sleep(Duration::from_millis(500));
    let after = (fds(), threads());
    eprintln!("F8-PERF descriptors and threads: before {before:?} after 300 connections {after:?}");
    assert!(
        after.0 < before.0 + 60,
        "descriptors grew from {} to {}",
        before.0,
        after.0
    );
    assert!(
        after.1 < before.1 + 40,
        "threads grew from {} to {}",
        before.1,
        after.1
    );
}

#[test]
fn the_lifetime_limit_of_sessions_is_a_fixed_immediate_refusal() {
    let app = App::start(Options {
        limit: 1,
        ..Options::default()
    });
    let mut created = app.gui.sessions(None).unwrap().capacity;
    let mut refused = None;
    let started = Instant::now();
    while refused.is_none() && created < 400 {
        match app.gui.prepare(uuid()) {
            Ok(_) => created += 1,
            Err(e) => refused = Some(e),
        }
    }
    let error = refused.expect("the authority refuses the session over its limit");
    assert_eq!(error.code, F8Failure::Busy, "{error:?}");
    assert_eq!(
        created, 128,
        "the authority holds 128 sessions over its lifetime"
    );
    eprintln!(
        "F8-PERF {created} sessions prepared in {:?}, then refused",
        started.elapsed()
    );
    // Reads still work, and nothing was added by the refusal.
    let head = app.head();
    assert!(app.gui.prepare(uuid()).is_err());
    assert_eq!(app.head(), head);
    assert_eq!(app.gui.sessions(None).unwrap().capacity, 128);
}

#[test]
#[ignore = "manual measurement: run alone, with --ignored --test-threads=1 --nocapture"]
fn the_table_of_what_the_application_costs() {
    let app = App::start(Options {
        limit: 1,
        ..Options::default()
    });
    app.knobs
        .queue
        .lock()
        .unwrap()
        .push_back(json!({"direct": true}));
    let baseline = (fds(), threads(), status_kib("/proc/self/status", "VmRSS:"));
    let s = app.up(AgentMode::Inference);
    let child = eventually(|| app.pid_file(&s.id, 1, "pid"));
    let idle = idle_cpu(&app, Duration::from_secs(10));
    let running = (fds(), threads(), status_kib("/proc/self/status", "VmRSS:"));
    eprintln!(
        "F8-TABLE baseline fds/threads/rssKiB {baseline:?} · with one runtime {running:?} · child rssKiB {} · idle authority thread {:.3}% of a core over 10 s",
        rss_kib(child),
        idle * 100.0
    );
    for _ in 0..500 {
        let mut raw = app.raw();
        raw.hello();
    }
    std::thread::sleep(Duration::from_secs(1));
    let after = (fds(), threads(), status_kib("/proc/self/status", "VmRSS:"));
    eprintln!("F8-TABLE after 500 connections fds/threads/rssKiB {after:?}");
    let stopped = app.gui.session(s.id.clone()).unwrap();
    app.gui
        .runtime_stop(
            uuid(),
            stopped.session.runtime.fence.clone(),
            stopped.session.runtime.revision,
        )
        .unwrap();
    app.until(&s.id, |d| d.session.runtime.actual == ActualState::Stopped);
    let ended = (fds(), threads(), status_kib("/proc/self/status", "VmRSS:"));
    eprintln!("F8-TABLE after the runtime stopped fds/threads/rssKiB {ended:?}");
}
