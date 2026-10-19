//! Planning against files laid out the ways real ones are: the bytes that are not the change
//! stay the bytes they were, a restoration of an untouched file is the original, and a
//! restoration into a file that moved on is checked against the file as it is.
use super::inventory::{SourceKey, item_id_for};
use super::model::Bounds;
use super::plan::{
    Drafted, Logical, PlanError, Prepared, apply_logical, draft_all_hooks, draft_park,
    draft_restore, draft_skill, hash, is_exactly,
};
use super::restore::{Level, Protection, RecordBody, RestoreError};
use serde_json::Value;

const SOURCE: SourceKey = SourceKey::ClaudeUserSettings;

fn id(event: &str, group: usize, index: usize) -> String {
    item_id_for(SOURCE, &[event, &group.to_string(), &index.to_string()])
}
fn sealed(_: bool) -> Result<Protection, RestoreError> {
    Ok(Protection::Sealed)
}
fn park(bytes: &str, event: &str, group: usize, index: usize) -> Result<Drafted, PlanError> {
    draft_park(
        bytes.as_bytes(),
        SOURCE,
        &id(event, group, index),
        &Bounds::PROPOSED,
        sealed,
    )
}
fn body(drafted: &Drafted) -> RecordBody {
    match &drafted.prepared {
        Prepared::Park { body, .. } => (**body).clone(),
        other => panic!("not a park: {other:?}"),
    }
}
fn text(drafted: &Drafted) -> String {
    String::from_utf8(drafted.new_bytes.clone()).unwrap()
}
fn restore(current: &str, drafted: &Drafted) -> Result<Drafted, PlanError> {
    let b = body(drafted);
    draft_restore(
        current.as_bytes(),
        "a".repeat(32).as_str(),
        &b,
        false,
        &Bounds::PROPOSED,
    )
}

/// Two-space layout, a few events, and a key order a printer would not keep.
const TWO_SPACES: &str = r#"{
  "model": "opus",
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          { "type": "command", "command": "first.sh" },
          { "type": "command", "command": "second.sh" }
        ]
      },
      {
        "matcher": "Edit",
        "hooks": [ { "type": "command", "command": "edit.sh" } ]
      }
    ],
    "Stop": [
      { "hooks": [ { "type": "command", "command": "stop.sh" } ] }
    ]
  },
  "effortLevel": "high"
}
"#;

#[test]
fn parking_one_of_two_handlers_removes_those_bytes_and_no_others() {
    let drafted = park(TWO_SPACES, "PreToolUse", 0, 0).unwrap();
    let after = text(&drafted);
    assert!(!after.contains("first.sh") && after.contains("second.sh"));
    // Everything outside the removed handler is byte for byte what it was.
    let expected = TWO_SPACES.replace(
        "          { \"type\": \"command\", \"command\": \"first.sh\" },\n",
        "",
    );
    assert_eq!(after, expected);
    assert_eq!(body(&drafted).level, Level::Handler);
    assert!(is_exactly(
        TWO_SPACES.as_bytes(),
        after.as_bytes(),
        &drafted.logical
    ));
}

#[test]
fn parking_the_last_handler_takes_the_comma_before_it() {
    let drafted = park(TWO_SPACES, "PreToolUse", 0, 1).unwrap();
    let expected = TWO_SPACES.replace(
        ",\n          { \"type\": \"command\", \"command\": \"second.sh\" }",
        "",
    );
    assert_eq!(text(&drafted), expected);
}

#[test]
fn parking_the_only_handler_of_a_group_removes_the_group() {
    let drafted = park(TWO_SPACES, "PreToolUse", 1, 0).unwrap();
    assert_eq!(body(&drafted).level, Level::Group);
    assert!(!text(&drafted).contains("edit.sh"));
    assert!(text(&drafted).contains("first.sh"));
    serde_json::from_str::<Value>(&text(&drafted)).unwrap();
}

#[test]
fn parking_the_only_hook_of_an_event_removes_the_event() {
    let drafted = park(TWO_SPACES, "Stop", 0, 0).unwrap();
    assert_eq!(body(&drafted).level, Level::Event);
    let after: Value = serde_json::from_str(&text(&drafted)).unwrap();
    assert!(after["hooks"].get("Stop").is_none());
    assert!(after["hooks"].get("PreToolUse").is_some());
    assert_eq!(after["effortLevel"], "high");
}

#[test]
fn parking_the_only_hook_of_the_file_leaves_an_empty_hooks_object() {
    let one = "{\n  \"hooks\": {\n    \"Stop\": [\n      { \"hooks\": [ { \"type\": \"command\", \"command\": \"x\" } ] }\n    ]\n  }\n}\n";
    let drafted = park(one, "Stop", 0, 0).unwrap();
    let after: Value = serde_json::from_str(&text(&drafted)).unwrap();
    assert_eq!(after, serde_json::json!({ "hooks": {} }));
    // And back, byte for byte.
    let back = restore(&text(&drafted), &drafted).unwrap();
    assert_eq!(text(&back), one);
}

#[test]
fn a_restoration_into_an_untouched_file_reproduces_the_original_bytes() {
    // AC-F2a-02, for every handler of the fixture and for three layouts of it.
    let compact =
        serde_json::to_string(&serde_json::from_str::<Value>(TWO_SPACES).unwrap()).unwrap();
    let tabs = TWO_SPACES.replace("  ", "\t");
    let crlf = TWO_SPACES.replace('\n', "\r\n");
    for layout in [TWO_SPACES, compact.as_str(), tabs.as_str(), crlf.as_str()] {
        for (event, group, index) in [
            ("PreToolUse", 0, 0),
            ("PreToolUse", 0, 1),
            ("PreToolUse", 1, 0),
            ("Stop", 0, 0),
        ] {
            let Ok(parked) = park(layout, event, group, index) else {
                // Item ids are positional: a layout without that position has none.
                panic!("{event} {group} {index} not found in {layout:?}");
            };
            let back = restore(&text(&parked), &parked).unwrap();
            assert_eq!(text(&back), layout, "{event} {group} {index}");
            assert_eq!(back.h1, hash(layout.as_bytes()));
        }
    }
}

#[test]
fn a_restoration_into_a_file_that_moved_on_is_planned_against_it() {
    let parked = park(TWO_SPACES, "PreToolUse", 0, 0).unwrap();
    // The user edited the file meanwhile.
    let moved = text(&parked).replace("\"effortLevel\": \"high\"", "\"effortLevel\": \"low\"");
    let back = restore(&moved, &parked).unwrap();
    let value: Value = serde_json::from_str(&text(&back)).unwrap();
    assert_eq!(value["effortLevel"], "low");
    let commands: Vec<&str> = value["hooks"]["PreToolUse"][0]["hooks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["command"].as_str().unwrap())
        .collect();
    // Its neighbour is still there; it goes back at the end of the group.
    assert_eq!(commands, ["second.sh", "first.sh"]);
    assert!(is_exactly(moved.as_bytes(), &back.new_bytes, &back.logical));
}

#[test]
fn a_restoration_where_the_group_or_event_is_gone_makes_them_again() {
    let parked = park(TWO_SPACES, "Stop", 0, 0).unwrap();
    assert_eq!(body(&parked).level, Level::Event);
    let back = restore(&text(&parked), &parked).unwrap();
    assert!(text(&back).contains("stop.sh"));

    // A handler whose group was deleted by hand: the group is made again around it.
    let handler = park(TWO_SPACES, "PreToolUse", 0, 0).unwrap();
    let without_group = r#"{ "hooks": { "Stop": [] }, "x": 1 }"#;
    let back = restore(without_group, &handler).unwrap();
    let value: Value = serde_json::from_str(&text(&back)).unwrap();
    assert_eq!(value["hooks"]["PreToolUse"][0]["matcher"], "Bash");
    assert_eq!(
        value["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
        "first.sh"
    );
}

#[test]
fn an_entry_that_is_already_back_is_not_added_twice() {
    let parked = park(TWO_SPACES, "PreToolUse", 0, 0).unwrap();
    // It reappeared by an outside change.
    let back = restore(TWO_SPACES, &parked).unwrap();
    assert_eq!(back.new_bytes, TWO_SPACES.as_bytes());
    assert!(matches!(
        back.prepared,
        Prepared::Restore {
            already_present: true,
            ..
        }
    ));
}

#[test]
fn a_different_entry_with_the_same_command_in_its_place_is_a_conflict() {
    let parked = park(TWO_SPACES, "PreToolUse", 0, 0).unwrap();
    let other = text(&parked).replace(
        "{ \"type\": \"command\", \"command\": \"second.sh\" }",
        "{ \"type\": \"command\", \"command\": \"first.sh\", \"timeout\": 99 }",
    );
    assert_eq!(
        restore(&other, &parked).unwrap_err(),
        PlanError::RestoreConflict
    );
}

#[test]
fn risk_follows_what_the_hook_can_do() {
    use super::change::Risk;
    assert_eq!(
        park(TWO_SPACES, "PreToolUse", 0, 0).unwrap().risk,
        Risk::WeakensSecurity
    );
    assert_eq!(park(TWO_SPACES, "Stop", 0, 0).unwrap().risk, Risk::Normal);
    let parked = park(TWO_SPACES, "Stop", 0, 0).unwrap();
    assert_eq!(
        restore(&text(&parked), &parked).unwrap().risk,
        Risk::ExecutesCode
    );
    assert_eq!(
        draft_all_hooks(TWO_SPACES.as_bytes(), SOURCE, true)
            .unwrap()
            .risk,
        Risk::WeakensSecurity
    );
}

#[test]
fn a_hook_that_may_hold_a_secret_is_parked_only_if_it_can_be_sealed() {
    let secret = r#"{ "hooks": { "Stop": [ { "hooks": [
      { "type": "command", "command": "notify --token SEKRET-in-a-command" },
      { "type": "command", "command": "plain.sh" } ] } ] } }"#;
    let refuse_secrets = |secret_bearing: bool| {
        if secret_bearing {
            Err(RestoreError::NoKeyStoreForSecret)
        } else {
            Ok(Protection::PlainNoSecret)
        }
    };
    let id_of = |index: usize| id("Stop", 0, index);
    let b = Bounds::PROPOSED;
    assert_eq!(
        draft_park(secret.as_bytes(), SOURCE, &id_of(0), &b, refuse_secrets).unwrap_err(),
        PlanError::KeyStoreUnavailable
    );
    let plain = draft_park(secret.as_bytes(), SOURCE, &id_of(1), &b, refuse_secrets).unwrap();
    assert!(matches!(
        plain.prepared,
        Prepared::Park {
            protection: Protection::PlainNoSecret,
            secret_bearing: false,
            ..
        }
    ));
    let sealed_one = draft_park(secret.as_bytes(), SOURCE, &id_of(0), &b, sealed).unwrap();
    assert!(matches!(
        sealed_one.prepared,
        Prepared::Park {
            protection: Protection::Sealed,
            secret_bearing: true,
            ..
        }
    ));
    assert!(sealed_one.holds_hidden_values);
}

#[test]
fn what_a_plan_shows_is_redacted_and_never_the_file() {
    let secret = r#"{ "hooks": { "Stop": [ { "hooks": [
      { "type": "command", "command": "notify --token SEKRET-in-a-command" } ] } ] } }"#;
    let drafted = draft_park(
        secret.as_bytes(),
        SOURCE,
        &id("Stop", 0, 0),
        &Bounds::PROPOSED,
        sealed,
    )
    .unwrap();
    let shown = serde_json::to_string(&drafted.diff).unwrap() + &drafted.name;
    assert!(!shown.contains("SEKRET"), "{shown}");
    assert!(shown.contains("notify"));
    assert!(drafted.holds_hidden_values);
}

#[test]
fn a_plan_for_what_is_not_there_or_not_json_is_refused() {
    assert_eq!(
        park(TWO_SPACES, "Stop", 3, 0).unwrap_err(),
        PlanError::EntryGone
    );
    assert_eq!(
        park("{ not json", "Stop", 0, 0).unwrap_err(),
        PlanError::Malformed
    );
    assert_eq!(
        park(r#"{"a":1,"a":2}"#, "Stop", 0, 0).unwrap_err(),
        PlanError::Malformed
    );
    assert_eq!(
        park(r#"{"hooks":{}}"#, "Stop", 0, 0).unwrap_err(),
        PlanError::EntryGone
    );
}

#[test]
fn the_coarse_switch_is_set_and_taken_away_without_touching_the_rest() {
    let on = draft_all_hooks(TWO_SPACES.as_bytes(), SOURCE, true).unwrap();
    let value: Value = serde_json::from_str(&text(&on)).unwrap();
    assert_eq!(value["disableAllHooks"], true);
    assert!(text(&on).contains("  \"disableAllHooks\": true"));
    assert_eq!(
        draft_all_hooks(on.new_bytes.as_slice(), SOURCE, true).unwrap_err(),
        PlanError::AlreadyDone
    );
    let off = draft_all_hooks(on.new_bytes.as_slice(), SOURCE, false).unwrap();
    let value: Value = serde_json::from_str(&text(&off)).unwrap();
    assert!(value.get("disableAllHooks").is_none());
    assert_eq!(
        draft_all_hooks(TWO_SPACES.as_bytes(), SOURCE, false).unwrap_err(),
        PlanError::AlreadyDone
    );
    // An explicit false is replaced in place.
    let explicit = TWO_SPACES.replace("\"model\"", "\"disableAllHooks\": false, \"model\"");
    let on = draft_all_hooks(explicit.as_bytes(), SOURCE, true).unwrap();
    assert_eq!(text(&on), explicit.replace("false", "true"));
}

#[test]
fn a_skill_is_switched_off_and_on_by_its_override() {
    let item = item_id_for(SourceKey::ClaudeUserSkills, &["noisy"]);
    let file = "{\n  \"model\": \"opus\"\n}\n";
    let off = draft_skill(file.as_bytes(), &item, "noisy", true).unwrap();
    let value: Value = serde_json::from_str(&text(&off)).unwrap();
    assert_eq!(value["skillOverrides"]["noisy"], "off");
    assert_eq!(value["model"], "opus");
    let again = draft_skill(off.new_bytes.as_slice(), &item, "noisy", true).unwrap_err();
    assert_eq!(again, PlanError::AlreadyDone);
    let on = draft_skill(off.new_bytes.as_slice(), &item, "noisy", false).unwrap();
    let value: Value = serde_json::from_str(&text(&on)).unwrap();
    assert!(value["skillOverrides"].get("noisy").is_none());
    assert_eq!(
        draft_skill(file.as_bytes(), &item, "noisy", false).unwrap_err(),
        PlanError::AlreadyDone
    );
    // A second skill joins the object the first made.
    let two = draft_skill(off.new_bytes.as_slice(), &item, "other", true).unwrap();
    let value: Value = serde_json::from_str(&text(&two)).unwrap();
    assert_eq!(
        value["skillOverrides"],
        serde_json::json!({ "noisy": "off", "other": "off" })
    );
    // An override the editor cannot read the meaning of is left alone.
    let odd = r#"{ "skillOverrides": { "noisy": "user-invocable-only" } }"#;
    assert_eq!(
        draft_skill(odd.as_bytes(), &item, "noisy", true).unwrap_err(),
        PlanError::EntryChanged
    );
    assert_eq!(
        draft_skill(odd.as_bytes(), &item, "noisy", false).unwrap_err(),
        PlanError::EntryChanged
    );
}

#[test]
fn a_logical_edit_that_does_not_apply_is_none_and_a_wrong_result_is_caught() {
    let before: Value = serde_json::from_str(TWO_SPACES).unwrap();
    assert!(apply_logical(&before, &Logical::RemoveRoot { key: "nope".into() }).is_none());
    let parked = park(TWO_SPACES, "Stop", 0, 0).unwrap();
    // The check refuses bytes that are not the change: here, the unchanged file.
    assert!(!is_exactly(
        TWO_SPACES.as_bytes(),
        TWO_SPACES.as_bytes(),
        &parked.logical
    ));
    assert!(!is_exactly(TWO_SPACES.as_bytes(), b"{}", &parked.logical));
}
