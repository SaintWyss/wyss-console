use super::*;
use crate::m3::report::{ActionRecord, Build, Phase, TapSummary};
use crate::m3::usage::{Ledger, Provider};
use serde_json::json;

fn report(actions: Vec<(&str, ActionState)>) -> ProviderReport {
    ProviderReport {
        provider: Provider::Claude,
        profile: "claude_local_tools_v1".into(),
        phase: Phase::Ready,
        refusal: None,
        build: Build {
            sdk_version: None,
            cli_version: None,
        },
        offered: vec![],
        tap: TapSummary::default(),
        usage: Ledger::new(Provider::Claude).snapshot(),
        refused_proposals: 0,
        pending_approvals: 0,
        indeterminate_actions: 0,
        denied_attempts: 0,
        failure: None,
        provider_error: None,
        model: None,
        actions: actions
            .into_iter()
            .map(|(i, s)| ActionRecord {
                item: i.into(),
                tool: "run_command".into(),
                state: s,
                refusal: None,
            })
            .collect(),
    }
}

#[test]
fn a_runtime_goes_from_starting_to_ready_when_the_host_binds_it() {
    let mut m = FixtureModel::new();
    assert_eq!(m.phase(), "stopped");
    m.starting("claude_inference_v1");
    assert_eq!(m.apply(&json!({"kind":"runtime_ready"})), Reaction::None);
    assert_eq!(m.phase(), "starting");
    assert_eq!(
        m.apply(&json!({"kind":"models","models":[],"offers":[],"complete":true})),
        Reaction::Bind
    );
    assert_eq!(
        m.phase(),
        "starting",
        "not ready until the binding is confirmed"
    );
    m.apply(&json!({"kind":"binding_confirmed","operation_id":"x","provider_thread_id":"y"}));
    assert_eq!(m.phase(), "ready");
}

#[test]
fn models_before_readiness_do_not_bind() {
    let mut m = FixtureModel::new();
    m.starting("claude_inference_v1");
    assert_eq!(m.apply(&json!({"kind":"models"})), Reaction::None);
}

#[test]
fn streamed_content_builds_one_assistant_message_per_turn() {
    let mut m = FixtureModel::new();
    m.starting("claude_inference_v1");
    m.prompt("t1", "hello");
    m.apply(&json!({"kind":"turn_running","managed_turn_id":"t1"}));
    for part in ["It is ", "a console."] {
        m.apply(&json!({"kind":"content","managed_turn_id":"t1","text":part}));
    }
    m.apply(&json!({"kind":"turn_completed","managed_turn_id":"t1"}));
    let v = m.view(None);
    assert_eq!(
        v.conversation
            .iter()
            .map(|c| (c.role, c.text.as_str()))
            .collect::<Vec<_>>(),
        [("user", "hello"), ("assistant", "It is a console.")]
    );
    assert_eq!(v.turn.unwrap().state, "completed");
    // Another turn starts a new assistant message even right after.
    m.prompt("t2", "again");
    m.apply(&json!({"kind":"content","managed_turn_id":"t2","text":"yes"}));
    assert_eq!(m.view(None).conversation.len(), 4);
}

#[test]
fn terminal_states_are_mapped() {
    for (kind, state) in [
        ("turn_completed", "completed"),
        ("turn_failed", "failed"),
        ("turn_interrupted", "interrupted"),
    ] {
        let mut m = FixtureModel::new();
        m.prompt("t", "x");
        m.apply(&json!({"kind":kind,"managed_turn_id":"t"}));
        assert_eq!(m.view(None).turn.unwrap().state, state);
    }
}

fn required(item: &str, command: &str) -> Value {
    json!({"kind":"approval_required","approval_kind":"command_execution","provider_item_id":item,"action_fingerprint":"f".repeat(64),
           "structure":{},"action":{"kind":"command","command":command,"cwd":"/w","parsed_actions":[]}})
}

#[test]
fn a_held_action_is_shown_complete_with_its_decisions_only_while_pending() {
    let mut m = FixtureModel::new();
    m.starting("claude_local_tools_v1");
    m.apply(&json!({"kind":"binding_confirmed"}));
    m.apply(&required("claude-act-1", "cargo build --release"));
    let v = m.view(Some(report(vec![("claude-act-1", ActionState::Held)])));
    assert_eq!(v.approvals.len(), 1);
    let a = &v.approvals[0];
    assert_eq!(
        (a.state, a.decisions.clone()),
        ("pending", vec!["accept", "decline", "cancel"])
    );
    assert_eq!(a.action["command"], "cargo build --release");
    assert_eq!(a.fingerprint.len(), 64);
    for (s, shown) in [
        (ActionState::Running, "accepted"),
        (ActionState::Completed, "confirmed"),
        (ActionState::Declined, "declined"),
        (ActionState::Cancelled, "cancelled"),
        (ActionState::Failed, "failed"),
        (ActionState::Indeterminate, "indeterminate"),
        (ActionState::Refused, "refused"),
    ] {
        let v = m.view(Some(report(vec![("claude-act-1", s)])));
        assert_eq!(v.approvals[0].state, shown);
        assert!(
            v.approvals[0].decisions.is_empty(),
            "{shown} offers no decision"
        );
    }
}

#[test]
fn no_decision_is_offered_unless_the_runtime_is_ready() {
    let mut m = FixtureModel::new();
    m.starting("claude_local_tools_v1");
    m.apply(&required("claude-act-1", "ls"));
    assert!(
        m.view(Some(report(vec![("claude-act-1", ActionState::Held)])))
            .approvals[0]
            .decisions
            .is_empty()
    );
    m.apply(&json!({"kind":"binding_confirmed"}));
    assert_eq!(
        m.view(Some(report(vec![("claude-act-1", ActionState::Held)])))
            .approvals[0]
            .decisions
            .len(),
        3
    );
    m.failed("the bridge died");
    assert!(
        m.view(None).approvals.is_empty(),
        "nothing held survives a failure"
    );
}

#[test]
fn duplicates_and_floods_are_bounded() {
    let mut m = FixtureModel::new();
    m.apply(&required("a", "x"));
    m.apply(&required("a", "y"));
    assert_eq!(m.view(None).approvals.len(), 1);
    assert_eq!(m.view(None).approvals[0].action["command"], "x");
    for i in 0..100 {
        m.apply(&required(&format!("i{i}"), "z"));
    }
    assert_eq!(m.view(None).approvals.len(), MAX_HELD);
    for i in 0..500 {
        m.prompt(&format!("t{i}"), "p");
    }
    assert_eq!(m.view(None).conversation.len(), MAX_MESSAGES);
}

#[test]
fn failure_detail_is_bounded_and_the_label_always_says_fixture() {
    let mut m = FixtureModel::new();
    m.failed(&"x".repeat(1000));
    let v = m.view(None);
    assert_eq!(v.phase, "failed");
    assert_eq!(v.detail.unwrap().len(), 160);
    assert!(v.label.starts_with("FIXTURE HOST"));
    m.refused("managed_policy_present");
    assert_eq!(m.view(None).phase, "refused");
}

#[test]
fn unknown_notices_change_nothing() {
    let mut m = FixtureModel::new();
    let before = m.view(None);
    assert_eq!(m.apply(&json!({"kind":"mystery"})), Reaction::None);
    assert_eq!(m.apply(&json!({})), Reaction::None);
    assert_eq!(m.view(None), before);
}
