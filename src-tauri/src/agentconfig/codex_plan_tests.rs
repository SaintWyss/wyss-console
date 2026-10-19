//! F2b's planner on isolated fixtures: only the touched key changes, the inverse restores the
//! file byte for byte, and the prediction is labelled and computed from the same rules the
//! authority applies (the equality with the authority's own predicate is tested in the
//! desktop's glue, which may name it).
use super::codex_plan::{CodexOperation, CodexPlanError, Facts, Verdict, draft};
use serde_json::json;

const CONFIG: &str = r#"# Codex configuration — hand written
model = "gpt-6"   # keep

[features]
hooks = true   # pinned by policy too

# Skills the owner switched
[[skills.config]]
path = "/home/u/.codex/skills/alpha/SKILL.md"
enabled = false   # parked for now

[[skills.config]]
path = "/home/u/.codex/skills/beta"

[mcp_servers.wyss_session]
command = "wyss"
args = [
  "serve",   # comment inside
]

[mcp_servers.web]
url = "https://example.invalid"
enabled = false
"#;

const ALPHA: &str = "/home/u/.codex/skills/alpha/SKILL.md";
const BETA: &str = "/home/u/.codex/skills/beta";

fn text(d: &super::codex_plan::CodexDrafted) -> String {
    String::from_utf8(d.new_bytes.clone()).unwrap()
}

#[test]
fn a_skill_is_switched_by_its_enabled_key_alone() {
    let on = draft(CONFIG, None, CodexOperation::SkillOn, ALPHA).unwrap();
    assert_eq!(
        text(&on),
        CONFIG.replace(
            "enabled = false   # parked for now",
            "enabled = true   # parked for now"
        )
    );
    let off = draft(CONFIG, None, CodexOperation::SkillOff, BETA).unwrap();
    assert_eq!(
        text(&off),
        CONFIG.replace(
            "path = \"/home/u/.codex/skills/beta\"\n",
            "path = \"/home/u/.codex/skills/beta\"\nenabled = false\n"
        )
    );
}

#[test]
fn an_mcp_server_is_switched_by_its_enabled_key_alone() {
    let off = draft(CONFIG, None, CodexOperation::McpOff, "wyss_session").unwrap();
    assert_eq!(
        text(&off),
        CONFIG.replace(
            "  \"serve\",   # comment inside\n]\n",
            "  \"serve\",   # comment inside\n]\nenabled = false\n"
        )
    );
    let on = draft(CONFIG, None, CodexOperation::McpOn, "web").unwrap();
    assert_eq!(
        text(&on),
        CONFIG.replace("enabled = false\n", "enabled = true\n")
    );
}

#[test]
fn every_comment_and_blank_line_survives_and_the_inverse_gives_the_file_back() {
    for (op, name) in [
        (CodexOperation::SkillOn, ALPHA),
        (CodexOperation::SkillOff, BETA),
        (CodexOperation::McpOff, "wyss_session"),
        (CodexOperation::McpOn, "web"),
    ] {
        let drafted = draft(CONFIG, None, op, name).unwrap();
        let after = text(&drafted);
        for line in CONFIG
            .lines()
            .filter(|l| l.trim_start().starts_with('#') || l.is_empty())
        {
            assert!(after.lines().any(|l| l == line), "{op:?}: lost {line:?}");
        }
        assert_eq!(
            String::from_utf8(drafted.inverse.apply(&drafted.new_bytes)).unwrap(),
            CONFIG,
            "{op:?}: the inverse is not exact"
        );
    }
}

#[test]
fn what_is_already_so_or_not_there_or_not_a_boolean_is_refused() {
    assert_eq!(
        draft(CONFIG, None, CodexOperation::SkillOff, ALPHA).unwrap_err(),
        CodexPlanError::AlreadyDone
    );
    assert_eq!(
        draft(CONFIG, None, CodexOperation::SkillOn, BETA).unwrap_err(),
        CodexPlanError::AlreadyDone
    );
    assert_eq!(
        draft(CONFIG, None, CodexOperation::McpOff, "web").unwrap_err(),
        CodexPlanError::AlreadyDone
    );
    assert_eq!(
        draft(CONFIG, None, CodexOperation::SkillOff, "/nope").unwrap_err(),
        CodexPlanError::EntryGone
    );
    assert_eq!(
        draft(CONFIG, None, CodexOperation::McpOn, "nope").unwrap_err(),
        CodexPlanError::EntryGone
    );
    let odd = CONFIG.replace("enabled = false\n", "enabled = \"no\"\n");
    assert_eq!(
        draft(&odd, None, CodexOperation::McpOn, "web").unwrap_err(),
        CodexPlanError::EntryChanged
    );
    assert_eq!(
        draft("not toml [", None, CodexOperation::McpOn, "web").unwrap_err(),
        CodexPlanError::Unsupported
    );
}

#[test]
fn a_layout_the_editor_cannot_place_is_left_alone() {
    // The servers are defined through an inline table: a plan would have to guess.
    let inline = "mcp_servers = { web = { url = \"x\", enabled = false } }\n";
    assert_eq!(
        draft(inline, None, CodexOperation::McpOn, "web").unwrap_err(),
        CodexPlanError::Unsupported
    );
    let inline_skills = "skills.config = [ { path = \"/a\", enabled = false } ]\n";
    assert_eq!(
        draft(inline_skills, None, CodexOperation::SkillOn, "/a").unwrap_err(),
        CodexPlanError::Unsupported
    );
}

#[test]
fn the_name_of_what_is_planned_goes_through_redaction() {
    // Spelled in two halves so that the repository scan does not take it for a key.
    let secret = concat!(
        "/home/u/.codex/skills/sk",
        "-abcdefghijklmnopqrstuvwxyz012345"
    );
    let config = format!("[[skills.config]]\npath = \"{secret}\"\n");
    let drafted = draft(&config, None, CodexOperation::SkillOff, secret).unwrap();
    assert!(
        !drafted.name.contains("abcdefghijklmnop"),
        "{}",
        drafted.name
    );
}

#[test]
fn the_prediction_is_computed_on_the_resulting_definitions() {
    // The owner's case: the engine pinned on and no hook defined. Switching a skill or an
    // MCP server changes nothing about managed Inference.
    let drafted = draft(CONFIG, None, CodexOperation::SkillOn, ALPHA).unwrap();
    assert_eq!(drafted.prediction.before, Verdict::Available);
    assert!(!drafted.prediction.changes_anything());

    // With one hook defined (enabled by default), Inference is unavailable before and after:
    // the cause is the hook, and the plan does not pretend to fix it.
    let with_hook = format!(
        "{CONFIG}\n[[hooks.Stop]]\n[[hooks.Stop.hooks]]\ntype = \"command\"\ncommand = \"x\"\n"
    );
    let drafted = draft(&with_hook, None, CodexOperation::SkillOn, ALPHA).unwrap();
    assert_eq!(drafted.prediction.before, Verdict::ExternalHooks);
    assert_eq!(drafted.prediction.after, Verdict::ExternalHooks);

    // A server whose name Inference cannot turn off by name makes the profile unprovable,
    // whatever is switched.
    let bad = format!("{CONFIG}\n[mcp_servers.\"bad name!\"]\nurl = \"x\"\n");
    let drafted = draft(&bad, None, CodexOperation::SkillOn, ALPHA).unwrap();
    assert_eq!(drafted.prediction.after, Verdict::McpUnprovable);
}

#[test]
fn the_facts_the_authority_checks_are_read_from_both_places_a_hook_can_live() {
    let config =
        json!({ "hooks": { "Stop": [ { "hooks": [ { "type": "command", "command": "x" } ] } ] } });
    let file = json!({ "hooks": { "Stop": [ { "hooks": [ { "type": "command", "command": "y", "enabled": false } ] } ] } });
    let facts = Facts::from_files(&config, Some(&file));
    assert_eq!(facts.hook_sources, vec![vec![true], vec![false]]);
    assert_eq!(facts.verdict(), Verdict::ExternalHooks);
    let only_disabled = Facts::from_files(&json!({}), Some(&file));
    assert_eq!(only_disabled.verdict(), Verdict::Available);
    let many: serde_json::Map<String, serde_json::Value> =
        (0..65).map(|n| (format!("s{n}"), json!({}))).collect();
    assert_eq!(
        Facts::from_files(&json!({ "mcp_servers": many }), None).verdict(),
        Verdict::McpUnprovable
    );
}

/// The planner's edit against the installed Codex, in a temporary `CODEX_HOME`: `codex mcp
/// list` must show the switch the file now says, and the inverse must show the first state
/// again. By hand: `cargo test -- --ignored real_codex --nocapture`.
#[test]
#[ignore = "runs the installed Codex, in a temporary home"]
fn real_codex_reads_the_mcp_switch_the_planner_wrote() {
    let home = std::env::temp_dir().join(format!("wyss-codex-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&home).unwrap();
    let list = || {
        let out = std::process::Command::new("codex")
            .args(["mcp", "list"])
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env("HOME", std::env::var_os("HOME").unwrap())
            .env("CODEX_HOME", &home)
            .output()
            .expect("codex is installed");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let status = |listing: &str, name: &str| {
        listing
            .lines()
            .find(|l| l.starts_with(name))
            .map(|l| l.split_whitespace().any(|w| w == "enabled"))
    };
    let config = "# mine\n[mcp_servers.alpha]\ncommand = \"true\"   # keep\n\n[mcp_servers.beta]\ncommand = \"true\"\nenabled = false\n";
    std::fs::write(home.join("config.toml"), config).unwrap();
    assert_eq!(status(&list(), "alpha"), Some(true));
    assert_eq!(status(&list(), "beta"), Some(false));

    let off = draft(config, None, CodexOperation::McpOff, "alpha").unwrap();
    std::fs::write(home.join("config.toml"), &off.new_bytes).unwrap();
    assert_eq!(
        status(&list(), "alpha"),
        Some(false),
        "Codex must see the switch that was written"
    );
    let on = draft(config, None, CodexOperation::McpOn, "beta").unwrap();
    std::fs::write(home.join("config.toml"), &on.new_bytes).unwrap();
    assert_eq!(status(&list(), "beta"), Some(true));

    std::fs::write(home.join("config.toml"), off.inverse.apply(&off.new_bytes)).unwrap();
    assert_eq!(
        std::fs::read_to_string(home.join("config.toml")).unwrap(),
        config
    );
    assert_eq!(status(&list(), "alpha"), Some(true));
    let _ = std::fs::remove_dir_all(&home);
}
