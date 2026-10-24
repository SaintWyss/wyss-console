use super::*;

fn t(input: Option<u64>, output: Option<u64>) -> Tokens {
    Tokens {
        input,
        output,
        ..Tokens::default()
    }
}
fn ledger() -> Ledger {
    Ledger::new(Provider::Claude)
}

#[test]
fn a_result_is_final_and_provisional_messages_are_discarded() {
    let mut l = ledger();
    l.begin_turn("t1");
    // The first report of a streaming message carries the provider's starting output count.
    l.message("t1", Some("m1"), Some("opus"), t(Some(11), Some(1)));
    l.result(
        "t1",
        t(Some(11), Some(7)),
        BTreeMap::new(),
        Some(0.0002),
        false,
    );
    let s = l.snapshot();
    assert_eq!(s.turns.len(), 1);
    assert_eq!(s.turns[0].total.output, Some(7));
    assert_eq!(s.turns[0].completeness, Completeness::Complete);
    assert_eq!(s.turns[0].estimate, Some(SdkEstimate { usd: 0.0002 }));
    assert_eq!(s.running.output.sum, 7);
}

#[test]
fn a_repeated_message_report_replaces_never_adds() {
    let mut l = ledger();
    l.begin_turn("t1");
    for _ in 0..5 {
        l.message("t1", Some("m1"), Some("opus"), t(Some(10), Some(1)));
    }
    l.message("t1", Some("m2"), Some("opus"), t(Some(20), Some(2)));
    l.end_turn(Completeness::Interrupted);
    let turn = &l.snapshot().turns[0];
    assert_eq!(turn.total.input, Some(30));
    assert_eq!(turn.total.output, Some(3));
    assert_eq!(turn.completeness, Completeness::Interrupted);
}

#[test]
fn unknown_is_never_zero() {
    let mut l = ledger();
    l.begin_turn("t1");
    l.message("t1", Some("m1"), None, Tokens::default());
    l.end_turn(Completeness::Interrupted);
    let turn = &l.snapshot().turns[0];
    assert_eq!(turn.total, Tokens::default());
    assert_eq!(turn.completeness, Completeness::Unreported);
    assert!(turn.total.is_unreported());
    let s = l.snapshot();
    assert_eq!(s.running.input.unreported_turns, 1);
    assert_eq!(s.running.input.sum, 0);
    assert!(!s.running.input.is_exact());
}

#[test]
fn a_partially_reported_turn_keeps_unreported_fields_unknown() {
    let mut l = ledger();
    l.begin_turn("t1");
    l.result("t1", t(Some(5), None), BTreeMap::new(), None, false);
    let s = l.snapshot();
    assert_eq!(s.turns[0].total.output, None);
    assert_eq!(s.running.input.reported_turns, 1);
    assert_eq!(s.running.output.unreported_turns, 1);
    assert!(s.running.input.is_exact());
    assert!(!s.running.output.is_exact());
}

#[test]
fn running_totals_are_per_counter_and_exact_only_when_every_turn_reported() {
    let mut l = ledger();
    for (i, o) in [(Some(1), Some(10)), (Some(2), None), (Some(3), Some(30))] {
        let id = format!("t{}", l.snapshot().turns.len());
        l.begin_turn(&id);
        l.result(&id, t(i, o), BTreeMap::new(), None, false);
    }
    let s = l.snapshot();
    assert_eq!((s.running.input.sum, s.running.input.is_exact()), (6, true));
    assert_eq!(
        (s.running.output.sum, s.running.output.is_exact()),
        (40, false)
    );
    assert_eq!(s.running.output.unreported_turns, 1);
}

#[test]
fn a_provider_error_result_is_labelled_not_hidden() {
    let mut l = ledger();
    l.begin_turn("t1");
    l.result("t1", t(Some(3), Some(0)), BTreeMap::new(), None, true);
    assert_eq!(
        l.snapshot().turns[0].completeness,
        Completeness::ProviderError
    );
}

#[test]
fn a_result_for_another_turn_is_ignored() {
    let mut l = ledger();
    l.begin_turn("t1");
    l.result("other", t(Some(1), Some(1)), BTreeMap::new(), None, false);
    l.message("other", Some("m"), None, t(Some(1), Some(1)));
    let s = l.snapshot();
    assert_eq!(s.turns.len(), 1);
    assert_eq!(s.turns[0].completeness, Completeness::Running);
    assert!(s.turns[0].total.is_unreported());
}

#[test]
fn a_new_turn_closes_the_previous_one_as_unreported_never_as_zero() {
    let mut l = ledger();
    l.begin_turn("t1");
    l.begin_turn("t2");
    let s = l.snapshot();
    assert_eq!(s.turns[0].turn_id, "t1");
    assert_eq!(s.turns[0].completeness, Completeness::Unreported);
    assert_eq!(s.turns[1].completeness, Completeness::Running);
}

#[test]
fn rerouted_and_helper_models_are_all_reported() {
    let mut l = ledger();
    l.begin_turn("t1");
    l.message(
        "t1",
        Some("m1"),
        Some("claude-sonnet-5-5"),
        t(Some(1), Some(1)),
    );
    l.message(
        "t1",
        Some("m2"),
        Some("claude-haiku-5-5"),
        t(Some(1), Some(1)),
    );
    assert_eq!(
        l.reported_models().into_iter().collect::<Vec<_>>(),
        ["claude-haiku-5-5", "claude-sonnet-5-5"]
    );
    l.end_turn(Completeness::Interrupted);
    assert_eq!(l.snapshot().turns[0].by_model.len(), 2);
}

#[test]
fn the_reported_model_is_what_the_provider_echoed_not_the_clients_bookkeeping() {
    let mut l = ledger();
    l.begin_turn("t1");
    // The provider's response names haiku...
    l.message(
        "t1",
        Some("m1"),
        Some("claude-haiku-5-5"),
        t(Some(1), Some(1)),
    );
    // ...while the SDK's per-model usage is keyed by the model it asked for.
    let mut by_model = BTreeMap::new();
    by_model.insert("claude-sonnet-5-5".to_owned(), t(Some(1), Some(1)));
    l.result("t1", t(Some(1), Some(1)), by_model, None, false);
    let turn = &l.snapshot().turns[0];
    assert_eq!(turn.echoed_models, ["claude-haiku-5-5"]);
    assert!(turn.by_model.contains_key("claude-sonnet-5-5"));
    assert_eq!(
        l.reported_models().into_iter().collect::<Vec<_>>(),
        ["claude-haiku-5-5"]
    );
}

#[test]
fn without_any_response_echo_the_sdks_bookkeeping_is_the_weaker_fallback() {
    let mut l = ledger();
    l.begin_turn("t1");
    let mut by_model = BTreeMap::new();
    by_model.insert("claude-sonnet-5-5".to_owned(), t(Some(1), Some(1)));
    l.result("t1", t(Some(1), Some(1)), by_model, None, false);
    assert!(l.snapshot().turns[0].echoed_models.is_empty());
    assert_eq!(
        l.reported_models().into_iter().collect::<Vec<_>>(),
        ["claude-sonnet-5-5"]
    );
}

#[test]
fn history_is_bounded_and_dropped_turns_stay_counted_in_the_running_total() {
    let mut l = ledger();
    for i in 0..(MAX_TURNS + 10) {
        let id = format!("t{i}");
        l.begin_turn(&id);
        l.result(&id, t(Some(1), Some(1)), BTreeMap::new(), None, false);
    }
    let s = l.snapshot();
    assert_eq!(s.turns.len(), MAX_TURNS);
    assert_eq!(s.dropped_turns, 10);
    assert_eq!(s.running.input.sum, (MAX_TURNS + 10) as u64);
    assert_eq!(s.turns[0].turn_id, "t10");
}

#[test]
fn a_message_flood_is_bounded() {
    let mut l = ledger();
    l.begin_turn("t1");
    for i in 0..10_000 {
        l.message(
            "t1",
            Some(&format!("m{i}")),
            Some("opus"),
            t(Some(1), Some(1)),
        );
        l.message("t1", None, Some("opus"), t(Some(1), Some(1)));
    }
    l.end_turn(Completeness::Interrupted);
    let turn = &l.snapshot().turns[0];
    assert!(turn.total.input.unwrap() <= 2 * 256);
}

#[test]
fn limits_are_replaced_per_window_and_bounded() {
    let mut l = ledger();
    let obs = |w, u| LimitObservation {
        window: w,
        status: LimitStatus::AllowedWarning,
        resets_at: Some(1),
        utilization: Some(u),
        observed_at: 5,
    };
    l.limit(obs(LimitWindow::FiveHour, 0.5));
    l.limit(obs(LimitWindow::FiveHour, 0.9));
    l.limit(obs(LimitWindow::SevenDay, 0.1));
    let s = l.snapshot();
    assert_eq!(s.limits.len(), 2);
    assert_eq!(s.limits[0].utilization, Some(0.9));
}

#[test]
fn parsing_limit_names_is_closed() {
    assert_eq!(LimitWindow::parse("five_hour"), Some(LimitWindow::FiveHour));
    assert_eq!(LimitWindow::parse("hourly"), None);
    assert_eq!(LimitStatus::parse("rejected"), Some(LimitStatus::Rejected));
    assert_eq!(LimitStatus::parse(""), None);
}

#[test]
fn non_finite_estimates_are_discarded() {
    let mut l = ledger();
    l.begin_turn("t1");
    l.result(
        "t1",
        t(Some(1), Some(1)),
        BTreeMap::new(),
        Some(f64::NAN),
        false,
    );
    assert_eq!(l.snapshot().turns[0].estimate, None);
    l.begin_turn("t2");
    l.result(
        "t2",
        t(Some(1), Some(1)),
        BTreeMap::new(),
        Some(-1.0),
        false,
    );
    assert_eq!(l.snapshot().turns[1].estimate, None);
}

#[test]
fn the_snapshot_serializes_without_any_content_field() {
    let mut l = ledger();
    l.begin_turn("t1");
    l.result("t1", t(Some(1), Some(2)), BTreeMap::new(), None, false);
    let json = serde_json::to_string(&l.snapshot()).unwrap();
    for forbidden in ["text", "prompt", "reasoning_text", "thinking", "content"] {
        assert!(
            !json.contains(&format!("\"{forbidden}\"")),
            "{forbidden}: {json}"
        );
    }
    assert!(json.contains("\"provider\":\"claude\""));
}
