use super::*;

fn sched() -> Scheduler<u64> {
    let mut s = Scheduler::new().unwrap();
    s.add_runtime("a").unwrap();
    s.add_runtime("b").unwrap();
    s
}
fn limits() -> Limits {
    Limits::load().unwrap()
}

#[test]
fn advertised_data_room_reserves_bytes_for_every_maximal_frame() {
    let mut s = sched();
    let l = limits();
    let frame_max = l.data_bytes / 3;
    s.push("a", Class::Data, frame_max, 1, 0).unwrap();
    let room = s.data_room("a", frame_max);
    assert_eq!(room, 2);
    for _ in 0..room {
        s.push("a", Class::Data, frame_max, 2, 0).unwrap();
    }
    assert_eq!(s.data_room("a", frame_max), 0);
    // Zero-sized messages are bounded by event slots, with no division by zero.
    assert_eq!(s.data_room("b", 0), l.data_events);
    assert_eq!(s.data_room("missing", frame_max), 0);
}

#[test]
fn lanes_are_bounded_and_back_pressure_is_explicit_never_a_growing_queue() {
    let mut s = sched();
    let l = limits();
    for i in 0..l.data_events {
        s.push("a", Class::Data, 1, i as u64, 0).unwrap();
    }
    assert_eq!(
        s.push("a", Class::Data, 1, 99, 0),
        Err(Refusal::Backpressure)
    );
    // The byte bound binds independently of the event count.
    let mut s = sched();
    s.push("a", Class::Data, l.data_bytes - 1, 0, 0).unwrap();
    assert_eq!(
        s.push("a", Class::Data, 2, 1, 0),
        Err(Refusal::Backpressure)
    );
    assert_eq!(
        s.push("a", Class::Data, l.data_bytes + 1, 1, 0),
        Err(Refusal::TooLarge)
    );
    assert_eq!(
        s.push("a", Class::Control, l.control_event_bytes + 1, 1, 0),
        Err(Refusal::TooLarge)
    );
    assert_eq!(
        s.push("zzz", Class::Data, 1, 0, 0),
        Err(Refusal::UnknownRuntime)
    );
    assert_eq!(s.queued_bytes("a").unwrap().0, l.data_bytes - 1);
    assert!(s.memory_ceiling(1) <= 524288 + 32768);
}

#[test]
fn a_data_flood_in_a_cannot_delay_or_evict_bs_reserved_control() {
    let mut s = sched();
    let l = limits();
    // A is flooded continuously; B asks for a revocation in the middle of it.
    let mut b_dispatched = None;
    let mut a_per_tick = Vec::new();
    for tick in 0..6u64 {
        loop {
            if s.push("a", Class::Data, 100, tick, tick * 10).is_err() {
                break;
            }
        }
        if tick == 2 {
            s.push("b", Class::Control, 64, 777, tick * 10).unwrap();
            s.push("b", Class::Data, 100, 5, tick * 10).unwrap();
        }
        let out = s.tick(tick * 10);
        a_per_tick.push(out.iter().filter(|d| d.runtime == "a").count());
        if let Some(d) = out.iter().find(|d| d.item == 777) {
            b_dispatched = Some((tick, d.waited_ms, d.late, d.class));
        }
        // B's data is served in the very quantum it is queued, despite A's flood.
        if tick == 2 {
            assert!(
                out.iter()
                    .any(|d| d.runtime == "b" && d.class == Class::Data)
            );
        }
    }
    // Control for B went out in the first quantum after it was queued: zero wait, not late.
    assert_eq!(b_dispatched, Some((2, 0, false, Class::Control)));
    // A never receives more than the per-quantum data allowance.
    assert!(a_per_tick.iter().all(|n| *n <= l.data_per_quantum));
    // And A's lane never exceeded its bound however hard it was pushed.
    assert!(s.queued_bytes("a").unwrap().0 <= l.data_bytes);
}

#[test]
fn control_has_reserved_capacity_but_overflow_faults_only_its_own_runtime() {
    let mut s = sched();
    let l = limits();
    // Data fully saturated does not consume control capacity.
    while s.push("a", Class::Data, 1, 0, 0).is_ok() {}
    for i in 0..l.control_events {
        s.push("a", Class::Control, 1, i as u64, 0).unwrap();
    }
    // One more control message cannot be dropped silently: A is faulted, B is untouched.
    s.push("b", Class::Data, 1, 1, 0).unwrap();
    assert_eq!(
        s.push("a", Class::Control, 1, 99, 0),
        Err(Refusal::ControlOverflow)
    );
    assert!(s.is_faulted("a") && !s.is_faulted("b"));
    assert_eq!(s.push("a", Class::Data, 1, 0, 0), Err(Refusal::Faulted));
    let out = s.tick(1);
    assert!(out.iter().all(|d| d.runtime == "b"));
    assert_eq!(out.len(), 1);
}

#[test]
fn round_robin_rotation_gives_every_runtime_equal_progress_under_equal_load() {
    let mut s: Scheduler<u64> = Scheduler::new().unwrap();
    for k in ["a", "b", "c"] {
        s.add_runtime(k).unwrap();
    }
    let mut served = BTreeMap::new();
    for tick in 0..30u64 {
        for k in ["a", "b", "c"] {
            let _ = s.push(k, Class::Data, 1, tick, tick);
        }
        for d in s.tick(tick) {
            *served.entry(d.runtime).or_insert(0usize) += 1;
        }
    }
    let counts: Vec<usize> = served.values().copied().collect();
    assert_eq!(counts.len(), 3);
    assert!(
        counts.iter().max().unwrap() - counts.iter().min().unwrap() <= limits().data_per_quantum
    );
}

#[test]
fn a_blocked_worker_is_faulted_alone_and_others_keep_progressing() {
    let mut s = sched();
    let l = limits();
    s.push("a", Class::Control, 8, 1, 0).unwrap();
    s.push("a", Class::Data, 8, 2, 0).unwrap();
    s.push("b", Class::Control, 8, 3, 0).unwrap();
    let out = s.tick(0);
    assert_eq!(out.len(), 3);
    // B acknowledges; A never does (blocked or crashed worker).
    s.ack("b", 1, 0);
    assert!(
        s.check_stalls(l.control_deadline_ms).is_empty(),
        "exactly at the deadline is not past it"
    );
    s.push("a", Class::Data, 8, 4, 10).unwrap();
    let faults = s.check_stalls(l.control_deadline_ms + 1);
    assert_eq!(
        faults,
        vec![Fault {
            runtime: "a".into(),
            reason: FaultReason::WorkerStalled,
            discarded: 1
        }]
    );
    assert!(s.is_faulted("a") && !s.is_faulted("b"));
    // B continues; A's remaining work is discarded, never delivered late.
    s.push("b", Class::Control, 8, 5, 300).unwrap();
    let out = s.tick(300);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].runtime, "b");
    // Removing the faulted runtime releases it; a fresh runtime of that name starts clean.
    assert_eq!(s.remove_runtime("a"), 0);
    s.add_runtime("a").unwrap();
    s.push("a", Class::Data, 1, 6, 301).unwrap();
}

#[test]
fn late_control_is_reported_and_clock_regression_cannot_hide_it() {
    let mut s = sched();
    let l = limits();
    s.push("a", Class::Control, 8, 1, 0).unwrap();
    let out = s.tick(l.control_deadline_ms + 50);
    assert!(out[0].late && out[0].waited_ms == l.control_deadline_ms + 50);
    // A tick whose clock precedes the queue time reports zero wait rather than underflowing.
    s.push("a", Class::Control, 8, 2, 1_000).unwrap();
    let out = s.tick(500);
    assert_eq!(out[0].waited_ms, 0);
}

#[test]
fn explicit_cancellation_discards_only_data_and_keeps_reserved_control() {
    let mut s = sched();
    s.push("a", Class::Data, 5, 1, 0).unwrap();
    s.push("a", Class::Data, 5, 2, 0).unwrap();
    s.push("a", Class::Control, 5, 3, 0).unwrap();
    assert_eq!(s.cancel_data("a"), 2);
    assert_eq!(s.queued_bytes("a"), Some((0, 5)));
    let out = s.tick(0);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].class, Class::Control);
    assert_eq!(s.add_runtime("a"), Err(Refusal::Duplicate));
}
