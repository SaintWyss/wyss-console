use super::*;
use crate::f8::audit::budget_key_valid;

#[test]
fn every_contract_ceiling_is_classified_and_violations_use_only_contract_keys() {
    let keys = contract().unwrap();
    assert!(keys.len() > 30);
    for key in keys.keys() {
        assert!(classify(key).is_some(), "unclassified ceiling {key}");
        assert!(budget_key_valid(key));
    }
    // Unknown names are not budget keys.
    assert!(classify("made_up").is_none() && !budget_key_valid("made_up"));
    // Anything reported as an enforced violation must be a key this meter enforces.
    for key in [
        "provider_frame_bytes",
        "turn_output_bytes",
        "turn_events",
        "turn_events_per_second",
        "turn_wall_seconds",
        "pending_approvals_per_runtime",
    ] {
        assert!(matches!(classify(key), Some(Enforcement::Enforced(_))));
    }
    // Resident memory is a soft observation of the root, never an enforcement.
    assert_eq!(
        classify("owned_root_rss_soft_kib"),
        Some(Enforcement::SoftObservation)
    );
    assert_eq!(classify("max_clients"), Some(Enforcement::NotProven));
}

#[test]
fn each_ceiling_fails_with_its_exact_key_and_latches() {
    let c = contract().unwrap();
    let mut m = Meter::new(0).unwrap();
    assert_eq!(
        m.charge_event(0, c["provider_frame_bytes"] + 1)
            .unwrap_err()
            .key,
        "provider_frame_bytes"
    );
    // Latched: later valid events keep failing with the first reason.
    assert_eq!(
        m.charge_event(10, 1).unwrap_err().key,
        "provider_frame_bytes"
    );

    // Output volume: one event at the frame size repeatedly, spaced so the rate never trips.
    let mut m = Meter::new(0).unwrap();
    let frame = c["provider_frame_bytes"];
    let per = c["turn_output_bytes"] / frame;
    for i in 0..per {
        m.charge_event(i * 10, frame).unwrap();
    }
    assert_eq!(
        m.charge_event(per * 10, frame).unwrap_err().key,
        "turn_output_bytes"
    );

    // Event count, with ample time between events.
    let mut m = Meter::new(0).unwrap();
    for i in 0..c["turn_events"] {
        m.charge_event(i * 10, 1)
            .unwrap_or_else(|v| panic!("{} at {i}", v.key));
    }
    assert_eq!(
        m.charge_event(c["turn_events"] * 10, 1).unwrap_err().key,
        "turn_events"
    );

    // Burst then rate: burst events at one instant pass, the next one fails.
    let mut m = Meter::new(0).unwrap();
    for _ in 0..c["turn_event_burst"] {
        m.charge_event(0, 1).unwrap();
    }
    assert_eq!(
        m.charge_event(0, 1).unwrap_err().key,
        "turn_events_per_second"
    );

    // Wall clock.
    let mut m = Meter::new(5_000).unwrap();
    m.check_wall(5_000 + c["turn_wall_seconds"] * 1000).unwrap();
    assert_eq!(
        m.check_wall(5_001 + c["turn_wall_seconds"] * 1000)
            .unwrap_err()
            .key,
        "turn_wall_seconds"
    );

    // Pending approvals.
    let mut m = Meter::new(0).unwrap();
    for _ in 0..c["pending_approvals_per_runtime"] {
        m.approval_opened().unwrap();
    }
    assert_eq!(
        m.approval_opened().unwrap_err().key,
        "pending_approvals_per_runtime"
    );
}

#[test]
fn the_token_bucket_refills_at_the_contract_rate_and_a_clock_cannot_run_backwards() {
    let c = contract().unwrap();
    let mut m = Meter::new(1_000).unwrap();
    for _ in 0..c["turn_event_burst"] {
        m.charge_event(1_000, 1).unwrap();
    }
    // A clock which moves backwards grants no tokens.
    assert!(m.charge_event(0, 1).is_err());
    let mut m = Meter::new(1_000).unwrap();
    for _ in 0..c["turn_event_burst"] {
        m.charge_event(1_000, 1).unwrap();
    }
    // One second later exactly `turn_events_per_second` events fit again.
    for _ in 0..c["turn_events_per_second"] {
        m.charge_event(2_000, 1).unwrap();
    }
    assert_eq!(
        m.charge_event(2_000, 1).unwrap_err().key,
        "turn_events_per_second"
    );
}

#[test]
fn a_violation_in_one_runtime_cannot_touch_another_meter() {
    let c = contract().unwrap();
    let mut a = Meter::new(0).unwrap();
    let mut b = Meter::new(0).unwrap();
    b.approval_opened().unwrap();
    assert!(a.charge_event(0, c["provider_frame_bytes"] + 1).is_err());
    assert!(a.violation().is_some());
    // B's approval and counters are exactly as they were, and B keeps working.
    assert!(b.violation().is_none());
    b.charge_event(1, 10).unwrap();
    b.approval_closed();
    b.approval_opened().unwrap();
}

#[test]
fn resident_memory_is_a_soft_observation_that_never_becomes_a_violation() {
    let c = contract().unwrap();
    let soft = c["owned_root_rss_soft_kib"];
    assert!(!observe_root_rss(soft).unwrap().over_soft_limit);
    assert!(observe_root_rss(soft + 1).unwrap().over_soft_limit);
    // Observing never touches a meter.
    let m = Meter::new(0).unwrap();
    let _ = observe_root_rss(u64::MAX).unwrap();
    assert!(m.violation().is_none());
}
