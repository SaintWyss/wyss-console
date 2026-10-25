//! M5 track A: the engine at more than one runtime, through the real application with a real
//! child process per runtime (the fixture child of the F8 acceptance suite; not Codex, so nothing
//! here says anything about a provider).
//!
//! Production capacity is one and stays one: these tests raise the bound only where the registry
//! allows it, inside this test build (`Registry::with_test_limit`). What they check is what
//! AC-FP-SES-04 asks of a higher capacity before anyone may ask for it: identities that cannot
//! cross, refusal before allocation with no hidden queue, capacity retained until the owned child
//! is reaped, a stalled or crashed provider that cannot hold a sibling's status, cancel, decline,
//! revoke or stop, a flood that cannot starve a sibling's control operations, and nothing left
//! behind after cycles. The measurement test is `#[ignore]`d: it prints the table, and its figures
//! are measurements, not budgets.
use super::*;
use std::{collections::BTreeSet, thread};

const COUNTS: [usize; 2] = [2, 4];

/// Brings up `n` sessions at once: a runtime's readiness does not wait for another's.
fn up_all(app: &App, n: usize, mode: AgentMode) -> Vec<Session> {
    thread::scope(|scope| {
        let started: Vec<_> = (0..n).map(|_| scope.spawn(|| app.up(mode))).collect();
        started.into_iter().map(|h| h.join().unwrap()).collect()
    })
}

fn child_pid(app: &App, s: &Session) -> u32 {
    eventually(|| app.pid_file(&s.id, 1, "pid"))
}

/// The pid is a direct child of this test process (so it is ours to signal), whatever a pid file says.
fn is_our_child(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| {
            let rest = s[s.rfind(')')? + 2..].to_owned();
            rest.split_whitespace().nth(1)?.parse::<u32>().ok()
        })
        == Some(std::process::id())
}

fn signal(pid: u32, signal: nix::sys::signal::Signal) {
    assert!(is_our_child(pid), "{pid} is not a child of this test");
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), signal).unwrap();
}

fn slowest(samples: &[Duration]) -> Duration {
    samples.iter().copied().max().unwrap_or_default()
}

// ---------------------------------------------------------------------------------- identity

#[test]
fn n_runtimes_run_side_by_side_and_no_identity_of_one_reaches_another() {
    for n in COUNTS {
        let app = App::start(Options {
            limit: n,
            tools: true,
            ..Options::default()
        });
        let sessions = up_all(&app, n, AgentMode::LocalTools);
        let instances: BTreeSet<String> = sessions
            .iter()
            .map(|s| format!("{:?}", s.fence.instance))
            .collect();
        assert_eq!(instances.len(), n, "one runtime instance each");
        assert_eq!(app.knobs.spawns.load(Ordering::Acquire), n);
        for s in &sessions {
            assert_eq!(s.fence.generation, 1);
        }
        let revisions: Vec<_> = sessions
            .iter()
            .map(|s| app.gui.session(s.id.clone()).unwrap().session.revision)
            .collect();
        let frames: Vec<_> = sessions.iter().map(|s| app.frames(&s.id, 1)).collect();
        let mut raw = app.raw();
        raw.hello();
        // Every ordered pair: the fence of one, the session of another, in every operation that
        // takes both.
        for (i, a) in sessions.iter().enumerate() {
            for (j, b) in sessions.iter().enumerate() {
                if i == j {
                    continue;
                }
                let turn = raw.send(
                    "managedSession/turn/start",
                    json!({"intent_id":uuid().as_str(),"managed_session_id":b.id.as_str(),
                        "expected_session_revision":revisions[j],"prompt":"work","runtime_fence":wire(&a.fence)}),
                );
                assert_eq!(
                    Raw::code(&turn),
                    Some("invalid_request"),
                    "{i}->{j}: {turn}"
                );
                let grant = raw.send(
                    "executionLease/grant",
                    json!({"intent_id":uuid().as_str(),"session_id":b.id.as_str(),
                        "expected_session_revision":revisions[j],"scopes":["turn_start"],
                        "ttl_seconds":60,"runtime_fence":wire(&a.fence)}),
                );
                assert_eq!(
                    Raw::code(&grant),
                    Some("invalid_request"),
                    "{i}->{j}: {grant}"
                );
                let bind = raw.send(
                    "managedSession/runtime/create",
                    json!({"intent_id":uuid().as_str(),"managed_session_id":b.id.as_str(),
                        "expected_session_revision":revisions[j],"workspace_root":app.workspace(),
                        "runtime_fence":wire(&a.fence)}),
                );
                assert_refused(&bind, &["gate_denied", "stale_revision", "invalid_request"]);
            }
        }
        for (k, s) in sessions.iter().enumerate() {
            assert_eq!(
                app.frames(&s.id, 1),
                frames[k],
                "no frame reached runtime {k}"
            );
            assert_eq!(app.sent(&s.id, 1, "turn/start"), 0);
            let now = app.gui.session(s.id.clone()).unwrap();
            assert_eq!(now.session.revision, revisions[k], "session {k} unchanged");
            assert!(now.turns.is_empty());
        }
        // A runtime's own turn works, and only its own child hears of it.
        let first = &sessions[0];
        let (turn, _) = app.waiting(first);
        for other in &sessions[1..] {
            assert_eq!(app.sent(&other.id, 1, "turn/start"), 0);
        }
        assert_eq!(app.sent(&first.id, 1, "turn/start"), 1);
        drop(turn);
    }
}

// ---------------------------------------------------------------------------------- capacity

#[test]
fn over_n_is_refused_before_any_allocation_and_nothing_waits_in_a_queue() {
    for n in COUNTS {
        let app = App::start(Options {
            limit: n,
            ..Options::default()
        });
        let ids: Vec<_> = (0..n + 2).map(|_| app.session()).collect();
        // The first n start at the same moment.
        let gate = std::sync::Barrier::new(n);
        let answers: Vec<Value> = thread::scope(|scope| {
            let handles: Vec<_> = ids[..n]
                .iter()
                .map(|id| {
                    let gate = &gate;
                    let app = &app;
                    scope.spawn(move || {
                        let mut raw = app.raw();
                        raw.hello();
                        gate.wait();
                        raw.send("runtime/start", start_params(id))
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert!(answers.iter().all(|a| a["error"].is_null()), "{answers:?}");
        for id in &ids[..n] {
            app.until(id, |d| d.session.runtime.actual == ActualState::Ready);
        }
        assert_eq!(app.knobs.spawns.load(Ordering::Acquire), n);
        // Over the bound: immediate, repeatable, leaving no trace.
        let head = app.head();
        let mut raw = app.raw();
        raw.hello();
        let mut times = Vec::new();
        for _ in 0..20 {
            let started = Instant::now();
            let over = raw.send("runtime/start", start_params(&ids[n]));
            times.push(started.elapsed());
            assert_eq!(Raw::code(&over), Some("capacity_exceeded"), "{over}");
        }
        assert!(slowest(&times) < Duration::from_millis(250), "{times:?}");
        assert_eq!(app.head(), head, "refusals leave no audit");
        assert_eq!(app.knobs.spawns.load(Ordering::Acquire), n);
        // A slot frees when its child is reaped, and the refused session is *not* started by it:
        // nothing was waiting.
        let victim = &ids[0];
        let ready = app.until(victim, |d| d.session.runtime.actual == ActualState::Ready);
        app.gui
            .runtime_stop(
                uuid(),
                ready.session.runtime.fence.clone(),
                ready.session.runtime.revision,
            )
            .unwrap();
        app.until(victim, |d| d.session.runtime.actual == ActualState::Stopped);
        thread::sleep(Duration::from_millis(600));
        assert_eq!(
            app.knobs.spawns.load(Ordering::Acquire),
            n,
            "no hidden queue"
        );
        assert_eq!(
            app.gui
                .session(ids[n].clone())
                .unwrap()
                .session
                .runtime
                .generation,
            0,
            "the refused session never started"
        );
        // Only an explicit new request uses the freed slot, exactly once.
        let again = raw.send("runtime/start", start_params(&ids[n]));
        assert!(again["error"].is_null(), "{again}");
        let over = raw.send("runtime/start", start_params(&ids[n + 1]));
        assert_eq!(Raw::code(&over), Some("capacity_exceeded"), "{over}");
        assert_eq!(app.knobs.spawns.load(Ordering::Acquire), n + 1);
    }
}

#[test]
fn production_capacity_is_one_and_only_a_test_build_can_say_otherwise() {
    // The constructor the application uses yields the reviewed bound, with no way to ask for more.
    let authority = WireUuid::try_from(AUTH.to_owned()).unwrap();
    let mut registry = Registry::new(authority.clone()).unwrap();
    assert!(registry.has_free_slot());
    let reference = |n: u8| RuntimeRef {
        schema: 2,
        authority_instance_id: authority.clone(),
        managed_session_id: ManagedSessionId::parse(&format!(
            "11111111-1111-4111-8111-1111111111{n:02x}"
        ))
        .unwrap(),
        runtime_instance_id: crate::contracts::identities::AgentRuntimeInstanceId::parse(&format!(
            "00000000-0000-4000-8000-0000000000{n:02x}"
        ))
        .unwrap(),
        generation: 1,
    };
    registry.reserve(reference(1)).unwrap();
    assert!(!registry.has_free_slot());
    assert_eq!(registry.reserve(reference(2)), Err(E::CapacityExceeded));
    // The only way to a larger bound is a method that exists in test builds only.
    let source =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/f8/registry.rs"))
            .unwrap();
    let at = source
        .find("pub(crate) fn with_test_limit")
        .expect("the test constructor");
    assert!(
        source[..at].trim_end().ends_with("#[cfg(test)]"),
        "with_test_limit must be compiled in test builds only"
    );
    assert_eq!(
        source.matches("limit: 1,").count(),
        1,
        "one production bound"
    );
    for production in [
        "application.rs",
        "authority.rs",
        "engine.rs",
        "production.rs",
        "service.rs",
    ] {
        let path = format!("{}/src/f8/{production}", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            !text.contains("with_test_limit("),
            "{production} must not choose a bound"
        );
    }
}

// ---------------------------------------------------------------------------------- stalls

#[test]
fn a_stalled_provider_holds_no_siblings_status_cancel_decline_revoke_or_stop() {
    for n in COUNTS {
        let app = App::start(Options {
            limit: n,
            interrupt_answer_ms: Some(1_000),
            ..Options::default()
        });
        app.knobs.grace_ms.store(100, Ordering::Release);
        let sessions = up_all(&app, n, AgentMode::Inference);
        let pids: Vec<u32> = sessions.iter().map(|s| child_pid(&app, s)).collect();
        // Every runtime has a turn running.
        let turns: Vec<_> = sessions
            .iter()
            .map(|s| {
                let intent = uuid();
                settled(|| {
                    app.gui.turn_start(
                        intent.clone(),
                        s.fence.clone(),
                        s.detail.session.revision,
                        "work".into(),
                    )
                })
                .id
            })
            .collect();
        for (s, turn) in sessions.iter().zip(&turns) {
            eventually(|| {
                app.gui
                    .turn(turn.clone(), None, true)
                    .ok()
                    .filter(|r| !r.turn.turn.state.terminal())
                    .map(|_| s.id.clone())
            });
        }
        // The provider of the first one stops dead: it reads nothing, writes nothing, answers nothing.
        signal(pids[0], nix::sys::signal::Signal::SIGSTOP);
        // Every sibling is served in full while it is stopped.
        let mut served = Vec::new();
        for (s, turn) in sessions.iter().zip(&turns).skip(1) {
            let started = Instant::now();
            let read = app.gui.session(s.id.clone()).unwrap();
            assert_eq!(read.session.runtime.actual, ActualState::Ready);
            served.push(started.elapsed());
            let reading = app.gui.turn(turn.clone(), None, false).unwrap();
            let started = Instant::now();
            settled(|| {
                app.gui.turn_cancel(
                    uuid(),
                    s.fence.clone(),
                    turn.clone(),
                    reading.turn.turn.revision,
                )
            });
            eventually(|| {
                app.gui
                    .turn(turn.clone(), None, false)
                    .ok()
                    .filter(|r| r.turn.turn.state.terminal())
            });
            served.push(started.elapsed());
            let lease = app.gui.session(s.id.clone()).unwrap().lease.unwrap();
            let started = Instant::now();
            settled(|| {
                app.gui
                    .lease_revoke(uuid(), lease.id.clone(), lease.revision)
            });
            served.push(started.elapsed());
            let started = Instant::now();
            let now = app.gui.session(s.id.clone()).unwrap();
            app.gui
                .runtime_stop(
                    uuid(),
                    now.session.runtime.fence.clone(),
                    now.session.runtime.revision,
                )
                .unwrap();
            app.until(&s.id, |d| d.session.runtime.actual == ActualState::Stopped);
            served.push(started.elapsed());
        }
        assert!(
            slowest(&served) < Duration::from_secs(3),
            "n={n}: a stalled sibling held an operation: {served:?}"
        );
        // The stalled one: the user cancels its turn; the interrupt is dispatched and never
        // answered, so past the control deadline that runtime alone is faulted and stopped through
        // its owned handle (the stopped process cannot hear a signal it can catch, so it ends by
        // force), and only when it is reaped is its slot free.
        let reading = app.gui.turn(turns[0].clone(), None, false).unwrap();
        let asked = Instant::now();
        settled(|| {
            app.gui.turn_cancel(
                uuid(),
                sessions[0].fence.clone(),
                turns[0].clone(),
                reading.turn.turn.revision,
            )
        });
        app.until(&sessions[0].id, |d| {
            matches!(
                d.session.runtime.actual,
                ActualState::Stopped | ActualState::Failed
            )
        });
        assert!(
            asked.elapsed() < Duration::from_secs(8),
            "{:?}",
            asked.elapsed()
        );
        eventually(|| (!alive(pids[0]) || !is_our_child(pids[0])).then_some(()));
        let fresh = app.session();
        let mut raw = app.raw();
        raw.hello();
        let after = raw.send("runtime/start", start_params(&fresh));
        assert!(
            after["error"].is_null(),
            "the slot of the stalled runtime is free: {after}"
        );
    }
}

#[test]
fn a_crash_in_one_runtime_changes_nothing_of_the_others_and_retains_its_slot_until_reaped() {
    for n in COUNTS {
        let app = App::start(Options {
            limit: n,
            tools: true,
            ..Options::default()
        });
        // The first launch dies when its approval is accepted; the rest are ordinary.
        app.knobs
            .queue
            .lock()
            .unwrap()
            .push_back(json!({"crash_on_accept":true}));
        let mut sessions = Vec::new();
        for _ in 0..n {
            sessions.push(app.up(AgentMode::LocalTools));
        }
        let waiting: Vec<_> = sessions.iter().map(|s| app.waiting(s)).collect();
        let before: Vec<_> = waiting
            .iter()
            .map(|(_, a)| (a.approval.revision, a.approval.decidable))
            .collect();
        // A accepts; its child dies mid-decision.
        let _ = app.decide(&sessions[0], &waiting[0].1, ApprovalDecision::Accept);
        app.until(&sessions[0].id, |d| {
            d.session.runtime.actual != ActualState::Ready
        });
        // The others have the approvals and revisions they had, and still decide on their own.
        for (k, s) in sessions.iter().enumerate().skip(1) {
            let still = app
                .gui
                .approval(waiting[k].1.approval.id.clone(), None, None)
                .unwrap();
            assert_eq!(
                (still.approval.revision, still.approval.decidable),
                before[k],
                "runtime {k}"
            );
            assert_eq!(
                app.gui
                    .session(s.id.clone())
                    .unwrap()
                    .session
                    .runtime
                    .actual,
                ActualState::Ready
            );
            assert_eq!(app.sent(&s.id, 1, "approval"), 0);
        }
        let receipt =
            settled(|| app.decide(&sessions[1], &waiting[1].1, ApprovalDecision::Decline));
        assert_eq!(receipt.state, ApprovalState::Declined);
        eventually(|| (app.sent(&sessions[1].id, 1, "approval") == 1).then_some(()));
        for s in sessions.iter().skip(2) {
            assert_eq!(app.sent(&s.id, 1, "approval"), 0);
        }
        // The crashed runtime's slot comes back only once its child is gone; then exactly one
        // more runtime fits.
        app.until(&sessions[0].id, |d| {
            matches!(
                d.session.runtime.actual,
                ActualState::Stopped | ActualState::Failed
            )
        });
        let fresh = app.session();
        let mut raw = app.raw();
        raw.hello();
        let started = raw.send("runtime/start", start_params(&fresh));
        assert!(started["error"].is_null(), "{started}");
    }
}

// ---------------------------------------------------------------------------------- fairness

#[test]
fn a_flood_from_some_runtimes_does_not_starve_the_control_operations_of_another() {
    let n = 4;
    let app = App::start(Options {
        limit: n,
        ..Options::default()
    });
    // Three launches flood on every turn; the fourth is ordinary.
    {
        let mut queue = app.knobs.queue.lock().unwrap();
        for _ in 0..3 {
            queue.push_back(json!({"flood":16000,"flood_count":200}));
        }
    }
    let mut sessions = Vec::new();
    for _ in 0..n {
        sessions.push(app.up(AgentMode::Inference));
    }
    let quiet = sessions.pop().unwrap();
    for s in &sessions {
        let intent = uuid();
        settled(|| {
            app.gui.turn_start(
                intent.clone(),
                s.fence.clone(),
                s.detail.session.revision,
                "work".into(),
            )
        });
    }
    let mut reads = Vec::new();
    let mut turns = Vec::new();
    for round in 0..10 {
        let started = Instant::now();
        let seen = app.gui.session(quiet.id.clone()).unwrap();
        assert_eq!(
            seen.session.runtime.actual,
            ActualState::Ready,
            "round {round}"
        );
        reads.push(started.elapsed());
        // A full turn of its own, started and cancelled, while the others pour output.
        let started = Instant::now();
        let intent = uuid();
        let turn = settled(|| {
            app.gui.turn_start(
                intent.clone(),
                quiet.fence.clone(),
                app.gui.session(quiet.id.clone()).unwrap().session.revision,
                "work".into(),
            )
        })
        .id;
        let reading = eventually(|| app.gui.turn(turn.clone(), None, false).ok());
        settled(|| {
            app.gui.turn_cancel(
                uuid(),
                quiet.fence.clone(),
                turn.clone(),
                app.gui
                    .turn(turn.clone(), None, false)
                    .map_or(reading.turn.turn.revision, |t| t.turn.turn.revision),
            )
        });
        eventually(|| {
            app.gui
                .turn(turn.clone(), None, false)
                .ok()
                .filter(|r| r.turn.turn.state.terminal())
        });
        turns.push(started.elapsed());
    }
    assert!(
        slowest(&reads) < Duration::from_secs(1),
        "reads under a flood: {reads:?}"
    );
    assert!(
        slowest(&turns) < Duration::from_secs(5),
        "turns under a flood: {turns:?}"
    );
    // Whatever became of the flooders, it stayed with them.
    assert_eq!(
        app.gui
            .session(quiet.id.clone())
            .unwrap()
            .session
            .runtime
            .actual,
        ActualState::Ready
    );
}

// ---------------------------------------------------------------------------------- cycles

#[test]
fn start_stop_cycles_at_n_leave_no_child_no_descriptor_and_no_slot_behind() {
    for n in COUNTS {
        let app = App::start(Options {
            limit: n,
            ..Options::default()
        });
        let baseline = std::fs::read_dir("/proc/self/fd").unwrap().count();
        let mut launched = Vec::new();
        for cycle in 0..6 {
            let sessions = if cycle == 0 {
                up_all(&app, n, AgentMode::Inference)
            } else {
                // Later cycles start new sessions: the old ones stay stopped.
                up_all(&app, n, AgentMode::Inference)
            };
            for s in &sessions {
                launched.push(child_pid(&app, s));
            }
            thread::scope(|scope| {
                for s in &sessions {
                    let app = &app;
                    scope.spawn(move || {
                        let now = app.gui.session(s.id.clone()).unwrap();
                        app.gui
                            .runtime_stop(
                                uuid(),
                                now.session.runtime.fence.clone(),
                                now.session.runtime.revision,
                            )
                            .unwrap();
                        app.until(&s.id, |d| d.session.runtime.actual == ActualState::Stopped);
                    });
                }
            });
        }
        assert_eq!(app.knobs.spawns.load(Ordering::Acquire), n * 6);
        for pid in &launched {
            eventually(|| (!alive(*pid) || !is_our_child(*pid)).then_some(()));
        }
        // Slots are all free: n more fit.
        let again = up_all(&app, n, AgentMode::Inference);
        assert_eq!(again.len(), n);
        let fds = std::fs::read_dir("/proc/self/fd").unwrap().count();
        assert!(
            fds <= baseline + 4 * n + 24,
            "descriptors grew from {baseline} to {fds}"
        );
    }
}

// ---------------------------------------------------------------------------------- measurement

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// `cargo test --release --lib f8::acceptance::concurrency::measure -- --ignored --nocapture --test-threads=1`
#[test]
#[ignore = "measurement: prints the capacity qualification table"]
fn measure_runtimes_at_one_two_and_four() {
    for n in [1usize, 2, 4] {
        let mut ready = Vec::new();
        let mut rss = Vec::new();
        let mut cancel = Vec::new();
        let mut idle_cpu = Vec::new();
        for _ in 0..8 {
            let app = App::start(Options {
                limit: n,
                ..Options::default()
            });
            let started = Instant::now();
            let sessions = up_all(&app, n, AgentMode::Inference);
            ready.push(ms(started.elapsed()));
            let pids: Vec<u32> = sessions.iter().map(|s| child_pid(&app, s)).collect();
            rss.push(pids.iter().map(|p| resources::rss_kib(*p)).sum::<u64>() as f64 / 1024.0);
            // Idle: what the application's own thread costs with n idle runtimes.
            let tid = app.tid.load(Ordering::Acquire);
            let before = resources::thread_cpu(tid);
            thread::sleep(Duration::from_secs(2));
            idle_cpu.push((resources::thread_cpu(tid) - before) / 2.0 * 100.0);
            // Cancellation of one turn while the others each have one running.
            for s in &sessions {
                let intent = uuid();
                settled(|| {
                    app.gui.turn_start(
                        intent.clone(),
                        s.fence.clone(),
                        s.detail.session.revision,
                        "work".into(),
                    )
                });
            }
            let s = &sessions[0];
            let turn = eventually(|| {
                app.gui
                    .session(s.id.clone())
                    .ok()?
                    .turns
                    .first()
                    .map(|t| t.turn.id.clone())
            });
            let reading = eventually(|| app.gui.turn(turn.clone(), None, false).ok());
            let started = Instant::now();
            settled(|| {
                app.gui.turn_cancel(
                    uuid(),
                    s.fence.clone(),
                    turn.clone(),
                    reading.turn.turn.revision,
                )
            });
            eventually(|| {
                app.gui
                    .turn(turn.clone(), None, false)
                    .ok()
                    .filter(|r| r.turn.turn.state.terminal())
            });
            cancel.push(ms(started.elapsed()));
        }
        let table = |what: &str, unit: &str, mut v: Vec<f64>| {
            let raw = v.clone();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!(
                "\n{{\"measure\":\"{what}\",\"runtimes\":{n},\"unit\":\"{unit}\",\"n\":{},\"p50\":{:.2},\"p95\":{:.2},\"raw\":{:?}}}",
                v.len(),
                v[v.len() / 2],
                v[((v.len() as f64 * 0.95) as usize).min(v.len() - 1)],
                raw.iter()
                    .map(|x| (x * 100.0).round() / 100.0)
                    .collect::<Vec<_>>()
            );
        };
        table("all_runtimes_ready_after_start", "ms", ready);
        table("sum_of_children_rss", "MiB", rss);
        table("one_turn_cancelled_with_others_running", "ms", cancel);
        table(
            "application_thread_cpu_with_idle_runtimes",
            "percent_of_one_core",
            idle_cpu,
        );
    }
}

// ---------------------------------------------------------------------------------- the only slot

/// The defect this suite found: a provider that stopped answering after a cancel kept its turn
/// running, so the runtime could not be stopped, the session could not be closed and, at capacity
/// one, nothing else could start, until the turn's 30-minute wall clock. The cancel now starts a
/// clock of its own (`interrupt_answer_ms`) and a provider that does not end the turn within it is
/// stopped through its owned handle; only that runtime is touched.
#[test]
fn a_provider_that_never_answers_a_cancel_cannot_hold_the_only_slot() {
    let app = App::start(Options {
        limit: 1,
        interrupt_answer_ms: Some(800),
        ..Options::default()
    });
    app.knobs.grace_ms.store(100, Ordering::Release);
    let s = app.up(AgentMode::Inference);
    let pid = child_pid(&app, &s);
    let intent = uuid();
    let turn = settled(|| {
        app.gui.turn_start(
            intent.clone(),
            s.fence.clone(),
            s.detail.session.revision,
            "work".into(),
        )
    })
    .id;
    eventually(|| {
        app.gui
            .turn(turn.clone(), None, true)
            .ok()
            .filter(|r| !r.turn.turn.state.terminal())
    });
    signal(pid, nix::sys::signal::Signal::SIGSTOP);
    // Before the cancel there is nothing to wait for: the turn simply runs (it is the cancel
    // that the provider owes an answer to).
    thread::sleep(Duration::from_millis(1_200));
    assert_eq!(
        app.gui
            .session(s.id.clone())
            .unwrap()
            .session
            .runtime
            .actual,
        ActualState::Ready
    );
    let reading = app.gui.turn(turn.clone(), None, false).unwrap();
    let asked = Instant::now();
    settled(|| {
        app.gui.turn_cancel(
            uuid(),
            s.fence.clone(),
            turn.clone(),
            reading.turn.turn.revision,
        )
    });
    // The cancel alone leaves the user a way out: the runtime ends, the child is reaped, the turn
    // is no longer running and the only slot is free again.
    app.until(&s.id, |d| {
        matches!(
            d.session.runtime.actual,
            ActualState::Stopped | ActualState::Failed
        )
    });
    assert!(
        asked.elapsed() < Duration::from_secs(6),
        "{:?}",
        asked.elapsed()
    );
    eventually(|| (!alive(pid) || !is_our_child(pid)).then_some(()));
    let state = app
        .gui
        .turn(turn.clone(), None, false)
        .unwrap()
        .turn
        .turn
        .state;
    assert!(state.terminal(), "the turn is not left running: {state:?}");
    // What stopped it is on record under the contract's own key.
    assert!(
        app.events("runtime_exited") + app.events("runtime_failed") >= 1,
        "the stop is recorded"
    );
    let fresh = app.session();
    let mut raw = app.raw();
    raw.hello();
    let started = raw.send("runtime/start", start_params(&fresh));
    assert!(started["error"].is_null(), "the slot is free: {started}");
}
