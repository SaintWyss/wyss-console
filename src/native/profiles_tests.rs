use super::{
    catalogue::AgentId,
    profile::{Body, HooksMode, Permission, ProfileError, templates},
    profiles::ProfileStore,
};
use std::{fs, path::PathBuf};

fn store() -> (ProfileStore, PathBuf) {
    let dir = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("wyss-profiles-{}", uuid::Uuid::new_v4().simple()));
    (ProfileStore::open(dir.join("profiles")).unwrap(), dir)
}

fn body(name: &str) -> Body {
    Body {
        name: name.to_owned(),
        applies_to: vec![AgentId::ClaudeCode, AgentId::Codex],
        ..Body::default()
    }
}

/// Writes a profile fixture with version timestamps, hashes and a current version.
fn saved(dir: &std::path::Path, id: &str, versions: &[(u32, &Body)], current: u32) {
    let versions: Vec<_> = versions
        .iter()
        .map(|(n, body)| serde_json::json!({"n": n, "saved_at": 1_700_000_000u64, "hash": "0011223344556677", "body": body}))
        .collect();
    let text = serde_json::json!({"id": id, "current": current, "versions": versions});
    fs::write(
        dir.join("profiles").join(format!("{id}.json")),
        serde_json::to_vec_pretty(&text).unwrap(),
    )
    .unwrap();
}

#[test]
fn the_templates_are_one_per_permission_and_the_removed_one_is_unknown() {
    let (store, dir) = store();
    let ids: Vec<&str> = templates().iter().map(|(id, _)| *id).collect();
    assert_eq!(
        ids,
        [
            "template-as-configured",
            "template-review",
            "template-edits",
            "template-full-access",
            "template-strict"
        ]
    );
    let permission = |id: &str| store.resolve(id).unwrap().0.body.permission;
    assert_eq!(permission("template-as-configured"), None);
    assert_eq!(permission("template-review"), Some(Permission::ReadOnly));
    assert_eq!(permission("template-edits"), Some(Permission::Edits));
    assert_eq!(permission("template-full-access"), Some(Permission::Bypass));
    assert_eq!(permission("template-strict"), Some(Permission::Ask));
    let (edits, version) = store.resolve("template-edits").unwrap();
    assert_eq!(version, 0);
    assert_eq!(edits.body.name, "Accept edits");
    assert_eq!(edits.body.applies_to, vec![AgentId::ClaudeCode]);
    // No template carries anything but a permission: model, effort, hooks and skills are not
    // the Console's to pass at a launch.
    for (id, body) in templates() {
        assert!(
            body.model.is_none()
                && body.effort.is_none()
                && body.hooks.is_none()
                && body.skills_off.is_empty()
                && body.extends.is_none(),
            "{id}"
        );
    }
    assert_eq!(
        store.resolve("template-exploration").unwrap_err(),
        ProfileError::Unknown
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn each_permission_template_becomes_the_flags_its_agent_documents() {
    use super::profile::plan;
    let (store, dir) = store();
    let flags = |id: &str, agent: AgentId| {
        let (resolved, _) = store.resolve(id).unwrap();
        let applies = resolved.body.applies_to.contains(&agent);
        plan(&resolved, agent, applies)
    };
    for agent in [AgentId::ClaudeCode, AgentId::Codex] {
        let plain = flags("template-as-configured", agent);
        assert!(words(&plain).is_empty() && plain.applied.is_empty());
        assert!(plain.not_applicable.is_empty());
    }
    assert_eq!(
        words(&flags("template-strict", AgentId::ClaudeCode)),
        ["--permission-mode", "manual"]
    );
    assert_eq!(
        words(&flags("template-review", AgentId::ClaudeCode)),
        ["--permission-mode", "plan"]
    );
    assert_eq!(
        words(&flags("template-edits", AgentId::ClaudeCode)),
        ["--permission-mode", "acceptEdits"]
    );
    assert_eq!(
        words(&flags("template-full-access", AgentId::ClaudeCode)),
        ["--dangerously-skip-permissions"]
    );
    assert_eq!(
        words(&flags("template-strict", AgentId::Codex)),
        ["-s", "workspace-write", "-a", "on-request"]
    );
    assert_eq!(
        words(&flags("template-review", AgentId::Codex)),
        ["-s", "read-only", "-a", "on-request"]
    );
    assert_eq!(
        words(&flags("template-full-access", AgentId::Codex)),
        ["--dangerously-bypass-approvals-and-sandbox"]
    );
    // Accept edits was not written for Codex: nothing is passed, and the start is refused.
    let codex_edits = flags("template-edits", AgentId::Codex);
    assert!(words(&codex_edits).is_empty());
    assert!(!codex_edits.not_applicable.is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_profile_saved_before_they_were_removed_is_still_read_at_its_current_version() {
    let (store, dir) = store();
    let (parent_id, child_id) = ("a".repeat(32), "b".repeat(32));
    let mut parent = body("parent");
    parent.model = Some("opus".to_owned());
    parent.hooks = Some(HooksMode::Off);
    parent.skills_off = vec!["a".to_owned()];
    saved(&dir, &parent_id, &[(1, &parent)], 1);
    let mut first = body("child");
    first.permission = Some(Permission::Bypass);
    let mut child = body("child");
    child.extends = Some(parent_id.clone());
    child.model = Some("sonnet".to_owned());
    child.permission = Some(Permission::ReadOnly);
    child.skills_off = vec!["b".to_owned()];
    // The pointer decides, not the newest or the widest version on file.
    saved(&dir, &child_id, &[(1, &first), (2, &child)], 2);
    let (resolved, version) = store.resolve(&child_id).unwrap();
    assert_eq!(version, 2);
    assert_eq!(resolved.body.model.as_deref(), Some("sonnet"));
    assert_eq!(resolved.body.permission, Some(Permission::ReadOnly));
    assert_eq!(resolved.body.hooks, Some(HooksMode::Off));
    assert_eq!(resolved.body.skills_off, ["a", "b"]);
    assert_eq!(resolved.from["model"], "child");
    assert_eq!(resolved.from["hooks"], "parent");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn an_id_that_cannot_be_resolved_is_an_error_and_never_a_start_without_a_profile() {
    let (store, dir) = store();
    assert_eq!(
        store.resolve(&"0".repeat(32)).unwrap_err(),
        ProfileError::Unknown
    );
    assert_eq!(
        store.resolve("../registry").unwrap_err(),
        ProfileError::Unknown
    );
    let (a, b) = ("a".repeat(32), "b".repeat(32));
    // A file that names another id, or whose pointer names no version, is not a profile.
    saved(&dir, &a, &[(1, &body("a"))], 7);
    assert_eq!(store.resolve(&a).unwrap_err(), ProfileError::Unknown);
    // A parent that is gone, a cycle, and a chain deeper than eight.
    let mut orphan = body("orphan");
    orphan.extends = Some("c".repeat(32));
    saved(&dir, &a, &[(1, &orphan)], 1);
    assert_eq!(store.resolve(&a).unwrap_err(), ProfileError::ExtendsUnknown);
    let mut one = body("one");
    one.extends = Some(b.clone());
    let mut two = body("two");
    two.extends = Some(a.clone());
    saved(&dir, &a, &[(1, &one)], 1);
    saved(&dir, &b, &[(1, &two)], 1);
    assert_eq!(store.resolve(&a).unwrap_err(), ProfileError::ProfileCycle);
    let ids: Vec<String> = (0..9).map(|n| format!("{n}").repeat(32)).collect();
    for (n, id) in ids.iter().enumerate() {
        let mut link = body(&format!("d{n}"));
        link.extends = ids.get(n + 1).cloned();
        saved(&dir, id, &[(1, &link)], 1);
    }
    assert_eq!(store.resolve(&ids[0]).unwrap_err(), ProfileError::TooDeep);
    assert!(store.resolve(&ids[1]).is_ok());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_body_holds_closed_words_only() {
    // A word the schema does not have is not a field.
    let parsed: Result<Body, _> =
        serde_json::from_str(r#"{"name":"p","applies_to":["codex"],"command":"rm -rf /"}"#);
    assert!(parsed.is_err());
    let parsed: Result<Body, _> = serde_json::from_str(
        r#"{"name":"p","applies_to":["codex"],"permission":"bypass_permissions"}"#,
    );
    assert!(parsed.is_err(), "the dangerous mode is not a word");
    // A body without an effort key round-trips.
    let plain = body("p");
    let text = serde_json::to_string(&plain).unwrap();
    assert!(!text.contains("effort"), "{text}");
    assert_eq!(serde_json::from_str::<Body>(&text).unwrap(), plain);
}

fn resolved(effort: Option<&str>, model: Option<&str>) -> super::profile::Resolved {
    let mut b = body("p");
    b.effort = effort.map(str::to_owned);
    b.model = model.map(str::to_owned);
    super::profile::merge(&[("p".to_owned(), b)])
}
fn words(plan: &super::profile::LaunchPlan) -> Vec<String> {
    plan.words
        .iter()
        .map(|w| match w {
            super::profile::Word::Literal(s) => s.clone(),
            super::profile::Word::Overlay => "<overlay>".to_owned(),
        })
        .collect()
}

#[test]
fn an_effort_is_passed_the_way_each_agent_documents_and_claude_only_takes_its_own_levels() {
    use super::profile::plan;
    // Claude Code: its own flag, and only the levels its --help names.
    let claude = plan(
        &resolved(Some("high"), Some("opus")),
        AgentId::ClaudeCode,
        true,
    );
    assert_eq!(words(&claude), ["--model", "opus", "--effort", "high"]);
    assert!(
        claude
            .applied
            .iter()
            .any(|a| a.contains("Reasoning effort: high"))
    );
    for level in ["low", "medium", "high", "xhigh", "max"] {
        let p = plan(&resolved(Some(level), None), AgentId::ClaudeCode, true);
        assert_eq!(words(&p), ["--effort", level]);
    }
    let unknown = plan(&resolved(Some("minimal"), None), AgentId::ClaudeCode, true);
    assert!(words(&unknown).is_empty());
    assert!(
        unknown
            .not_applicable
            .iter()
            .any(|n| n.contains("\"minimal\" is not one of them"))
    );
    // Codex: no start flag of its own; the configuration key, for this session only.
    let codex = plan(
        &resolved(Some("minimal"), Some("gpt-x")),
        AgentId::Codex,
        true,
    );
    assert_eq!(
        words(&codex),
        ["-m", "gpt-x", "-c", "model_reasoning_effort=\"minimal\""]
    );
    assert!(
        codex
            .applied
            .iter()
            .any(|a| a.contains("this session only"))
    );
    // Nothing, when the profile asks for none or was not written for the agent.
    assert!(words(&plan(&resolved(None, None), AgentId::Codex, true)).is_empty());
    assert!(words(&plan(&resolved(Some("high"), None), AgentId::Codex, false)).is_empty());
}

#[test]
fn opencode_profiles_pass_only_the_v1194_model_flag_and_refuse_unqualified_controls() {
    let with_model = super::profile::plan(
        &resolved(None, Some("openai/gpt-test")),
        AgentId::Opencode,
        true,
    );
    assert_eq!(words(&with_model), ["--model", "openai/gpt-test"]);
    assert!(with_model.not_applicable.is_empty());

    let mut unsupported = body("OpenCode profile");
    unsupported.applies_to = vec![AgentId::Opencode];
    unsupported.permission = Some(Permission::ReadOnly);
    unsupported.hooks = Some(HooksMode::Off);
    unsupported.skills_off = vec!["skill".to_owned()];
    let unsupported = super::profile::plan(
        &super::profile::merge(&[("p".into(), unsupported)]),
        AgentId::Opencode,
        true,
    );
    assert_eq!(words(&unsupported), Vec::<String>::new());
    assert_eq!(unsupported.not_applicable.len(), 3);

    let hermes = super::profile::plan(
        &resolved(None, Some("provider/model")),
        AgentId::Hermes,
        true,
    );
    assert!(words(&hermes).is_empty());
    assert!(!hermes.not_applicable.is_empty());
}

#[test]
fn an_effort_is_inherited_like_a_model_and_the_nearer_profile_wins() {
    let mut parent = body("parent");
    parent.effort = Some("low".into());
    parent.model = Some("m-parent".into());
    let mut child = body("child");
    child.effort = Some("high".into());
    let merged = super::profile::merge(&[
        ("child".to_owned(), child),
        ("parent".to_owned(), parent.clone()),
    ]);
    assert_eq!(merged.body.effort.as_deref(), Some("high"));
    assert_eq!(merged.body.model.as_deref(), Some("m-parent"));
    assert_eq!(merged.from.get("effort").map(String::as_str), Some("child"));
    assert_eq!(merged.from.get("model").map(String::as_str), Some("parent"));
    let alone = super::profile::merge(&[
        ("child".to_owned(), body("child")),
        ("parent".to_owned(), parent),
    ]);
    assert_eq!(alone.body.effort.as_deref(), Some("low"));
}

#[test]
fn full_access_is_one_explicit_word_with_its_cost_said() {
    use super::profile::{BYPASS_COST, Resolved, plan};
    let mut full = body("Full");
    full.permission = Some(Permission::Bypass);
    let resolved = Resolved {
        body: full.clone(),
        from: Default::default(),
    };
    let claude = plan(&resolved, AgentId::ClaudeCode, true);
    assert_eq!(words(&claude), vec!["--dangerously-skip-permissions"]);
    assert!(claude.with_cost.contains(&BYPASS_COST.to_owned()));
    let codex = plan(&resolved, AgentId::Codex, true);
    assert_eq!(
        words(&codex),
        vec!["--dangerously-bypass-approvals-and-sandbox"]
    );
    assert!(codex.with_cost.contains(&BYPASS_COST.to_owned()));
    // Providers whose permissions are not managed get nothing, and are told so.
    let opencode = plan(&resolved, AgentId::Opencode, true);
    assert!(words(&opencode).is_empty());
    assert!(!opencode.not_applicable.is_empty());
    // The other words never reach full access.
    for other in [Permission::Ask, Permission::ReadOnly, Permission::Edits] {
        let mut b = body("p");
        b.permission = Some(other);
        let r = Resolved {
            body: b,
            from: Default::default(),
        };
        for agent in [AgentId::ClaudeCode, AgentId::Codex] {
            let w = words(&plan(&r, agent, true)).join(" ");
            assert!(!w.contains("dangerously"), "{other:?} {agent:?}: {w}");
        }
    }
    assert_eq!(
        serde_json::to_value(Permission::Bypass).unwrap(),
        serde_json::json!("bypass")
    );
}

#[test]
fn the_full_access_template_is_for_both_agents() {
    let (_, template) = templates()
        .into_iter()
        .find(|(id, _)| *id == "template-full-access")
        .expect("template");
    assert_eq!(template.name, "Full access (bypass)");
    assert_eq!(template.permission, Some(Permission::Bypass));
    assert_eq!(
        template.applies_to,
        vec![AgentId::ClaudeCode, AgentId::Codex]
    );
}
