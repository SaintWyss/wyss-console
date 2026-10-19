//! The OpenCode writer against disposable text. Every secret is the word `SEKRET`.
use super::change::Prior;
use super::inventory::SourceKey;
use super::model::Bounds;
use super::opencode_plan::{
    Logical, apply_logical, draft_mcp, draft_park_plugin, draft_restore_plugin, is_exactly,
    resolve_plugin, resolve_server,
};
use super::plan::PlanError;
use super::reader::parse_jsonc;

const KEY: SourceKey = SourceKey::OpencodeUserConfig;

const FILE: &str = r#"// my config
{
  "$schema": "https://opencode.ai/config.json",
  /* plugins */
  "plugin": [
    "file:///opt/a", // first
    "file:///opt/b",
    ["rtk.ts", { "x": 1 }]
  ],
  "provider": {
    "google": { "options": { "apiKey": "SEKRET-google" } }, // keep
  },
  "mcp": {
    "docs": { "type": "remote", "url": "https://h/x", "enabled": false },
    "local": { "type": "local", "command": ["srv"] },  // no enabled
  },
}
"#;

fn value(bytes: &[u8]) -> serde_json::Value {
    parse_jsonc(bytes, &Bounds::PROPOSED).unwrap().unwrap()
}

#[test]
fn an_mcp_switch_changes_only_its_own_bytes_and_undoes_exactly() {
    let off = draft_mcp(FILE.as_bytes(), "docs", true, None).unwrap();
    // `docs` is off: turning it on changes the one word.
    let expected = FILE.replacen(r#""enabled": false"#, r#""enabled": true"#, 1);
    assert_eq!(String::from_utf8(off.new_bytes.clone()).unwrap(), expected);
    assert_eq!(off.prior, Prior::Disabled);
    // And back, exactly.
    let back = draft_mcp(&off.new_bytes, "docs", false, Some(off.prior)).unwrap();
    assert_eq!(back.new_bytes, FILE.as_bytes());

    // No `enabled` yet: it is added, comments kept, and the undo removes just that.
    let added = draft_mcp(FILE.as_bytes(), "local", false, None).unwrap();
    let text = String::from_utf8(added.new_bytes.clone()).unwrap();
    assert!(
        text.contains(r#""command": ["srv"], "enabled": false"#),
        "{text}"
    );
    assert!(text.contains("// no enabled") && text.contains("// my config"));
    assert_eq!(added.prior, Prior::Absent);
    let undone = draft_mcp(&added.new_bytes, "local", true, Some(Prior::Absent)).unwrap();
    assert_eq!(undone.new_bytes, FILE.as_bytes());
}

#[test]
fn an_mcp_switch_refuses_what_it_cannot_place() {
    assert_eq!(
        draft_mcp(FILE.as_bytes(), "ghost", false, None).err(),
        Some(PlanError::EntryGone)
    );
    assert_eq!(
        draft_mcp(FILE.as_bytes(), "docs", false, None).err(),
        Some(PlanError::AlreadyDone)
    );
    let odd = FILE.replace(r#""enabled": false"#, r#""enabled": "no""#);
    assert_eq!(
        draft_mcp(odd.as_bytes(), "docs", true, None).err(),
        Some(PlanError::EntryChanged)
    );
    // A repeated key is ambiguous: no edit at all.
    let twice = r#"{ "mcp": { "a": { "enabled": true, "enabled": false } } }"#;
    assert_eq!(
        draft_mcp(twice.as_bytes(), "a", false, None).err(),
        Some(PlanError::Malformed)
    );
    // A server that moved on after a plan: the undo is not forced over it.
    assert_eq!(
        draft_mcp(FILE.as_bytes(), "local", true, Some(Prior::Enabled)).err(),
        Some(PlanError::EntryChanged)
    );
}

#[test]
fn a_plugin_is_taken_out_and_put_back_exactly_wherever_it_sat() {
    for index in 0..3 {
        let parked = draft_park_plugin(FILE.as_bytes(), KEY, index);
        let Ok(parked) = parked else {
            // The first entry is followed by a comment of its own on the same line, and the
            // stretch that goes would take it: refused, never taken along.
            assert_eq!(index, 0, "{parked:?}", parked = parked.err());
            continue;
        };
        let after = value(&parked.new_bytes);
        assert_eq!(after["plugin"].as_array().unwrap().len(), 2);
        assert_eq!(value(FILE.as_bytes())["provider"], after["provider"]);
        // Everything else is the same bytes: comments, the key, layout.
        let text = String::from_utf8(parked.new_bytes.clone()).unwrap();
        assert!(text.contains("// my config") && text.contains("SEKRET-google"));
        let restored = draft_restore_plugin(&parked.new_bytes, &parked.body).unwrap();
        assert_eq!(restored.new_bytes, FILE.as_bytes(), "index {index}");
    }
}

#[test]
fn a_plugin_goes_back_to_the_end_when_the_file_moved_on_and_is_not_added_twice() {
    let parked = draft_park_plugin(FILE.as_bytes(), KEY, 1).unwrap();
    // Somebody edited the file in between: not the exact bytes any more.
    let moved = String::from_utf8(parked.new_bytes.clone())
        .unwrap()
        .replace("\"google\"", "\"google2\"");
    let back = draft_restore_plugin(moved.as_bytes(), &parked.body).unwrap();
    assert!(!back.already_present);
    let after = value(&back.new_bytes);
    let list = after["plugin"].as_array().unwrap();
    assert_eq!(list.last().unwrap(), "file:///opt/b");
    assert!(
        String::from_utf8(back.new_bytes.clone())
            .unwrap()
            .contains("// my config")
    );
    assert!(is_exactly(moved.as_bytes(), &back.new_bytes, &back.logical));
    // Already back: nothing to write.
    let again = draft_restore_plugin(&back.new_bytes, &parked.body).unwrap();
    assert!(again.already_present);
    assert_eq!(again.new_bytes, back.new_bytes);
    // No list any more: it is created.
    let none = r#"{ "model": "a/b" }"#;
    let created = draft_restore_plugin(none.as_bytes(), &parked.body).unwrap();
    assert_eq!(
        value(&created.new_bytes)["plugin"],
        serde_json::json!(["file:///opt/b"])
    );
}

#[test]
fn a_removal_that_would_take_a_comment_is_refused_and_the_file_is_untouched() {
    let with = "{ \"plugin\": [\n  \"a\",\n  // belongs to b\n  \"b\"\n] }";
    // Removing `a` would remove the stretch up to `b`, comment included.
    assert_eq!(
        draft_park_plugin(with.as_bytes(), KEY, 0).err(),
        Some(PlanError::NotSupported)
    );
    // Removing `b` takes `,\n  // belongs to b\n  ` before it: refused too.
    assert_eq!(
        draft_park_plugin(with.as_bytes(), KEY, 1).err(),
        Some(PlanError::NotSupported)
    );
    let empty_with_comment = "{ \"plugin\": [ /* none */ \"only\" ] }";
    assert!(draft_park_plugin(empty_with_comment.as_bytes(), KEY, 0).is_err());
}

#[test]
fn plain_layouts_are_edited_in_their_own_style() {
    let one_line = r#"{"plugin":["a","b","c"],"model":"x"}"#;
    let parked = draft_park_plugin(one_line.as_bytes(), KEY, 1).unwrap();
    assert_eq!(
        String::from_utf8(parked.new_bytes.clone()).unwrap(),
        r#"{"plugin":["a","c"],"model":"x"}"#
    );
    let back = draft_restore_plugin(&parked.new_bytes, &parked.body).unwrap();
    assert_eq!(back.new_bytes, one_line.as_bytes());
    let only = r#"{ "plugin": ["a"], }"#;
    let parked = draft_park_plugin(only.as_bytes(), KEY, 0).unwrap();
    assert_eq!(value(&parked.new_bytes)["plugin"], serde_json::json!([]));
    let back = draft_restore_plugin(&parked.new_bytes, &parked.body).unwrap();
    assert_eq!(back.new_bytes, only.as_bytes());
}

#[test]
fn items_are_found_by_their_identifier_and_provider_options_are_never_read() {
    let id = super::inventory::item_id_for(KEY, &["file:///opt/b"]);
    assert_eq!(resolve_plugin(FILE.as_bytes(), KEY, &id).unwrap().0, 1);
    let rtk = super::inventory::item_id_for(KEY, &["rtk.ts"]);
    assert_eq!(resolve_plugin(FILE.as_bytes(), KEY, &rtk).unwrap().0, 2);
    let docs = super::inventory::item_id_for(KEY, &["docs"]);
    assert_eq!(resolve_server(FILE.as_bytes(), KEY, &docs).unwrap(), "docs");
    assert_eq!(
        resolve_server(FILE.as_bytes(), KEY, &"0".repeat(32)).err(),
        Some(PlanError::EntryGone)
    );
    // No plan, parked body or logical edit mentions the key.
    let parked = draft_park_plugin(FILE.as_bytes(), KEY, 1).unwrap();
    assert!(!format!("{:?}{:?}", parked.body, parked.logical).contains("SEKRET"));
    let mcp = draft_mcp(FILE.as_bytes(), "docs", true, None).unwrap();
    assert!(!format!("{:?}", mcp.logical).contains("SEKRET"));
}

#[test]
fn every_edit_is_checked_against_the_value_it_claims_to_make() {
    let before = value(FILE.as_bytes());
    assert!(apply_logical(&before, &Logical::RemovePlugin { index: 9 }).is_none());
    assert!(is_exactly(
        FILE.as_bytes(),
        FILE.as_bytes(),
        &Logical::Nothing
    ));
    let wrong = FILE.replace("file:///opt/b", "file:///opt/c");
    assert!(!is_exactly(
        FILE.as_bytes(),
        wrong.as_bytes(),
        &Logical::RemovePlugin { index: 1 }
    ));
}
